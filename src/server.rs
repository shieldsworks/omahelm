//! `omahelm serve`: the chart engine. It draws tiles into a cache for the
//! chartplotter window and answers what's charted at a point, over the
//! Unix socket in docs/protocol.md.

use crate::chart::{Chart, Geom, Item};
use crate::geo::{self, Rect, mercator};
use crate::library::{self, Fnv, Library};
use crate::marks;
use crate::render::{self, Style, TILE, TileKey};
use crate::s57::{self, names::*};
use crate::style::{self, Palette, Settings};
use crate::text::Font;
use serde_json::{Value, json};
use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::io::AsRawFd;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{SyncSender, TrySendError, sync_channel};
use std::sync::{Arc, Condvar, Mutex, RwLock};
use std::time::{Duration, Instant, SystemTime};

pub const VERSION: u32 = 1;
const MAX_LINE: usize = 64 * 1024;
const MAX_TILES: u64 = 256;
/// Messages a client may fall behind by before it's dropped.
const BACKLOG: usize = 1024;
/// Cells are let go after this long without a tile to draw.
const IDLE: Duration = Duration::from_secs(300);

static STOP: AtomicBool = AtomicBool::new(false);

extern "C" fn on_signal(_: libc::c_int) {
    STOP.store(true, Ordering::SeqCst);
}

pub fn socket_path() -> PathBuf {
    runtime_dir().join("helm.sock")
}

fn runtime_dir() -> PathBuf {
    let base =
        std::env::var("XDG_RUNTIME_DIR").map_or_else(|_| std::env::temp_dir(), PathBuf::from);
    base.join("omahelm")
}

fn cache_dir() -> PathBuf {
    let base = std::env::var("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| style::home().join(".cache"));
    base.join("omahelm/tiles")
}

struct Client {
    id: u64,
    tx: SyncSender<String>,
    stream: UnixStream,
}

struct Job {
    client: u64,
    key: TileKey,
    generation: String,
}

/// Everything that decides how tiles look, swapped whole when it changes.
struct View {
    library: Arc<Library>,
    style: Arc<Style>,
    generation: String,
    tiles: PathBuf,
    problems: Vec<String>,
    indexing: Option<(usize, usize)>,
}

struct Engine {
    view: RwLock<Arc<View>>,
    font: Option<Font>,
    clients: Mutex<Vec<Client>>,
    queue: Mutex<VecDeque<Job>>,
    ready: Condvar,
    busy: Mutex<Instant>,
    charts: PathBuf,
    next_id: AtomicU64,
}

fn mtime(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

/// What the watcher compares: the theme (through its symlink), the
/// settings and the chart index.
fn watched(charts: &Path) -> Vec<(PathBuf, Option<SystemTime>)> {
    let theme = style::theme_path();
    let theme = std::fs::canonicalize(&theme).unwrap_or(theme);
    [
        theme,
        style::config_path(),
        charts.join("index.json"),
        charts.join("ENC_ROOT"),
    ]
    .into_iter()
    .map(|p| {
        let t = mtime(&p);
        (p, t)
    })
    .collect()
}

fn load_style() -> (Style, Vec<String>) {
    let path = style::config_path();
    let (settings, problems) = match std::fs::read_to_string(&path) {
        Ok(text) => {
            let (s, p) = Settings::parse(&text);
            (
                s,
                p.into_iter().map(|p| format!("config.toml: {p}")).collect(),
            )
        }
        Err(_) => (Settings::default(), Vec::new()),
    };
    let palette = if settings.palette == "paper" {
        Palette::paper()
    } else {
        Palette::from_theme(&style::read_theme(&style::theme_path()))
    };
    (Style { palette, settings }, problems)
}

impl Engine {
    fn make_view(&self, library: Arc<Library>, indexing: Option<(usize, usize)>) -> View {
        let (style, mut problems) = load_style();
        if self.font.is_none() {
            problems.push("No font found: charts are drawn without soundings or names.".into());
        }
        let mut h = Fnv::default();
        h.write(style.key().as_bytes());
        h.write(library.fingerprint().as_bytes());
        h.write(
            self.font
                .as_ref()
                .map_or("", |f| f.file.as_str())
                .as_bytes(),
        );
        let generation = format!("{:016x}", h.0);
        View {
            tiles: cache_dir().join(&generation),
            library,
            style: Arc::new(style),
            generation,
            problems,
            indexing,
        }
    }

    fn state(&self) -> String {
        let view = self.view.read().expect("view lock").clone();
        let lib = &view.library;
        let set = &view.style.settings;
        let u = set.units;
        let round = |m: f64| (u.from_metres(m) * 10.0).round() / 10.0;
        let mut charts = json!({
            "status": if view.indexing.is_some() { "indexing" } else if lib.entries.is_empty() { "empty" } else { "ok" },
            "cells": lib.entries.len(),
            "skipped": lib.problems.len(),
            "root": self.charts.display().to_string(),
        });
        if let Some((done, total)) = view.indexing {
            charts["progress"] = json!({"done": done, "total": total});
        }
        if !lib.entries.is_empty() {
            let e = lib.extent();
            let (west, north) = geo::lon_lat([e.x0, e.y0]);
            let (east, south) = geo::lon_lat([e.x1, e.y1]);
            charts["extent"] = json!({"west": west, "south": south, "east": east, "north": north});
        }
        let mut root = view.tiles.display().to_string();
        root.push('/');
        let mut state = json!({
            "type": "state", "v": VERSION,
            "charts": charts,
            "tiles": {"root": root, "generation": view.generation},
            "settings": {
                "units": u.name(),
                "safetyDepth": round(set.safety_depth),
                "shallowContour": round(set.shallow_contour),
                "safetyContour": round(set.safety_contour),
                "deepContour": round(set.deep_contour),
                "palette": set.palette,
            },
        });
        if !view.problems.is_empty() {
            state["problems"] = json!(view.problems);
        }
        state.to_string()
    }

    fn send(&self, client: u64, line: String) {
        let mut clients = self.clients.lock().expect("clients lock");
        if let Some(i) = clients.iter().position(|c| c.id == client)
            && let Err(TrySendError::Full(_)) = clients[i].tx.try_send(line)
        {
            // Too far behind: drop it. It can reconnect.
            let _ = clients[i].stream.shutdown(std::net::Shutdown::Both);
            clients.remove(i);
        }
    }

    fn broadcast(&self, line: String) {
        let ids: Vec<u64> = self
            .clients
            .lock()
            .expect("clients lock")
            .iter()
            .map(|c| c.id)
            .collect();
        for id in ids {
            self.send(id, line.clone());
        }
    }

    fn set_view(&self, view: View) {
        let old = std::mem::replace(&mut *self.view.write().expect("view lock"), Arc::new(view));
        let new = self.view.read().expect("view lock").clone();
        if old.generation != new.generation {
            prune(&cache_dir(), &[&new.generation, &old.generation]);
            // Queued tiles of the old look are no use to anyone.
            self.queue
                .lock()
                .expect("queue lock")
                .retain(|j| j.generation == new.generation);
        }
        self.broadcast(self.state());
    }
}

/// Deletes tile generations other than the ones named.
fn prune(dir: &Path, keep: &[&str]) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        let name = e.file_name();
        let name = name.to_string_lossy();
        if !keep.contains(&name.as_ref())
            && name.len() == 16
            && name.chars().all(|c| c.is_ascii_hexdigit())
        {
            let _ = std::fs::remove_dir_all(e.path());
        }
    }
}

fn tile_path(key: &TileKey) -> String {
    format!("{}/{}/{}@{}.png", key.z, key.x, key.y, key.scale)
}

fn tile_message(key: &TileKey, path: Option<&str>, error: Option<&str>) -> String {
    let mut m = json!({"type": "tile", "v": VERSION, "z": key.z, "x": key.x, "y": key.y, "scale": key.scale});
    if let Some(p) = path {
        m["path"] = json!(p);
    }
    if let Some(e) = error {
        m["error"] = json!(e);
    }
    m.to_string()
}

fn error_message(message: &str) -> String {
    json!({"type": "error", "v": VERSION, "message": message}).to_string()
}

/// Holds the lock file for the life of the engine.
struct Lock {
    _file: std::fs::File,
}

fn lock(path: &Path) -> Result<Option<Lock>, String> {
    let f = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(path)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    // SAFETY: flock on a descriptor we own.
    let r = unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    Ok(if r == 0 {
        Some(Lock { _file: f })
    } else {
        None
    })
}

pub fn serve(charts: PathBuf) -> Result<(), String> {
    let dir = runtime_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let _ = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700));
    let socket = socket_path();
    let Some(_lock) = lock(&socket.with_extension("sock.lock"))? else {
        eprintln!("omahelm: already running on {}", socket.display());
        return Ok(());
    };
    let _ = std::fs::remove_file(&socket);
    let listener = UnixListener::bind(&socket).map_err(|e| format!("{}: {e}", socket.display()))?;
    let _ = std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600));
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    // SAFETY: the handler only stores to an atomic.
    unsafe {
        libc::signal(libc::SIGINT, on_signal as *const () as libc::sighandler_t);
        libc::signal(libc::SIGTERM, on_signal as *const () as libc::sighandler_t);
        libc::signal(libc::SIGPIPE, libc::SIG_IGN);
    }
    std::fs::create_dir_all(&charts).map_err(|e| format!("{}: {e}", charts.display()))?;

    let font = Font::load();
    let engine = Arc::new(Engine {
        view: RwLock::new(Arc::new(View {
            library: Arc::new(Library::empty(&charts)),
            style: Arc::new(load_style().0),
            generation: String::new(),
            tiles: cache_dir(),
            problems: Vec::new(),
            indexing: Some((0, 0)),
        })),
        font,
        clients: Mutex::new(Vec::new()),
        queue: Mutex::new(VecDeque::new()),
        ready: Condvar::new(),
        busy: Mutex::new(Instant::now()),
        charts: charts.clone(),
        next_id: AtomicU64::new(1),
    });
    let first = engine.make_view(Arc::new(Library::empty(&charts)), Some((0, 0)));
    engine.set_view(first);
    spawn_indexer(&engine);

    let workers = std::thread::available_parallelism()
        .map_or(2, |n| n.get())
        .clamp(1, 4);
    for _ in 0..workers {
        let e = engine.clone();
        std::thread::spawn(move || worker(&e));
    }
    {
        let e = engine.clone();
        std::thread::spawn(move || watcher(&e));
    }
    eprintln!(
        "omahelm: serving {} from {}",
        socket.display(),
        charts.display()
    );
    while !STOP.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok((stream, _)) => {
                let e = engine.clone();
                std::thread::spawn(move || connection(&e, stream));
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(e) => eprintln!("omahelm: accept: {e}"),
        }
    }
    let _ = std::fs::remove_file(&socket);
    Ok(())
}

/// Reads the chart index in the background, reporting progress.
fn spawn_indexer(engine: &Arc<Engine>) {
    let e = engine.clone();
    std::thread::spawn(move || {
        let last = Mutex::new(Instant::now());
        let lib = Library::open(&e.charts, &|done, total| {
            let mut last = last.lock().expect("progress lock");
            if last.elapsed() > Duration::from_millis(250) {
                *last = Instant::now();
                // The charts already on show stay until the new index is ready.
                let current = e.view.read().expect("view lock").library.clone();
                let v = e.make_view(current, Some((done, total)));
                e.set_view(v);
            }
        });
        let v = e.make_view(Arc::new(lib), None);
        e.set_view(v);
    });
}

fn watcher(engine: &Arc<Engine>) {
    let mut seen = watched(&engine.charts);
    loop {
        std::thread::sleep(Duration::from_secs(2));
        if STOP.load(Ordering::SeqCst) {
            return;
        }
        let now = watched(&engine.charts);
        if now != seen {
            let charts_changed = now[2..] != seen[2..];
            seen = now;
            if charts_changed {
                spawn_indexer(engine);
            } else {
                let lib = engine.view.read().expect("view lock").library.clone();
                let v = engine.make_view(lib, None);
                engine.set_view(v);
            }
        }
        let idle = engine.busy.lock().expect("busy lock").elapsed() > IDLE;
        if idle {
            engine.view.read().expect("view lock").library.forget();
        }
    }
}

fn worker(engine: &Arc<Engine>) {
    loop {
        let job = {
            let mut q = engine.queue.lock().expect("queue lock");
            loop {
                if let Some(j) = q.pop_front() {
                    break j;
                }
                q = engine.ready.wait(q).expect("queue lock");
            }
        };
        *engine.busy.lock().expect("busy lock") = Instant::now();
        let view = engine.view.read().expect("view lock").clone();
        if view.generation != job.generation {
            continue;
        }
        let rel = tile_path(&job.key);
        let file = view.tiles.join(&rel);
        let result = if file.exists() {
            Ok(())
        } else {
            render::render(&view.library, &view.style, engine.font.as_ref(), job.key)
                .and_then(|pm| render::png(&pm))
                .and_then(|bytes| write_atomic(&file, &bytes))
        };
        let message = match result {
            Ok(()) => tile_message(&job.key, Some(&rel), None),
            Err(e) => tile_message(&job.key, None, Some(&e)),
        };
        engine.send(job.client, message);
    }
}

fn write_atomic(file: &Path, bytes: &[u8]) -> Result<(), String> {
    let dir = file.parent().ok_or("bad tile path")?;
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let tmp = file.with_extension(format!("png.{}.tmp", std::process::id()));
    std::fs::write(&tmp, bytes).map_err(|e| format!("{}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, file).map_err(|e| format!("{}: {e}", file.display()))
}

fn connection(engine: &Arc<Engine>, stream: UnixStream) {
    let _ = stream.set_nonblocking(false);
    let id = engine.next_id.fetch_add(1, Ordering::SeqCst);
    let (tx, rx) = sync_channel::<String>(BACKLOG);
    let (Ok(writer), Ok(closer)) = (stream.try_clone(), stream.try_clone()) else {
        return;
    };
    engine.clients.lock().expect("clients lock").push(Client {
        id,
        tx,
        stream: closer,
    });
    std::thread::spawn(move || {
        let mut writer = writer;
        let _ = writer.set_write_timeout(Some(Duration::from_secs(2)));
        for line in rx {
            if writer
                .write_all(line.as_bytes())
                .and_then(|_| writer.write_all(b"\n"))
                .is_err()
            {
                let _ = writer.shutdown(std::net::Shutdown::Both);
                break;
            }
        }
    });
    engine.send(
        id,
        json!({"type": "hello", "v": VERSION, "helm": env!("CARGO_PKG_VERSION")}).to_string(),
    );
    engine.send(id, engine.state());

    let mut reader = BufReader::new(stream);
    let mut line = Vec::new();
    loop {
        line.clear();
        let n = match (&mut reader)
            .take(MAX_LINE as u64 + 1)
            .read_until(b'\n', &mut line)
        {
            Ok(0) | Err(_) => break,
            Ok(n) => n,
        };
        if n > MAX_LINE && line.last() != Some(&b'\n') {
            engine.send(id, error_message("line too long"));
            // Skip the rest of it.
            let mut rest = Vec::new();
            if reader.read_until(b'\n', &mut rest).unwrap_or(0) == 0 {
                break;
            }
            continue;
        }
        let reply = match serde_json::from_slice::<Value>(&line) {
            Ok(Value::Object(m)) => handle(engine, id, &Value::Object(m)),
            _ => Some(error_message("not a JSON object")),
        };
        if let Some(r) = reply {
            engine.send(id, r);
        }
    }
    engine
        .clients
        .lock()
        .expect("clients lock")
        .retain(|c| c.id != id);
    engine
        .queue
        .lock()
        .expect("queue lock")
        .retain(|j| j.client != id);
}

fn uint(v: &Value, key: &str) -> Option<u64> {
    v.get(key)?.as_u64()
}

fn handle(engine: &Arc<Engine>, id: u64, m: &Value) -> Option<String> {
    match m.get("type").and_then(Value::as_str) {
        Some("tiles") => tiles(engine, id, m)
            .err()
            .map(|e| error_message(&format!("tiles: {e}"))),
        Some("query") => Some(match query(engine, m) {
            Ok(v) => v.to_string(),
            Err(e) => error_message(&format!("query: {e}")),
        }),
        Some(other) => Some(error_message(&format!("unknown type {other}"))),
        None => Some(error_message("missing type")),
    }
}

fn tiles(engine: &Arc<Engine>, id: u64, m: &Value) -> Result<(), String> {
    let get = |k: &str| uint(m, k).ok_or_else(|| format!("{k} must be a whole number"));
    let (z, x0, y0, x1, y1, scale) = (
        get("z")?,
        get("x0")?,
        get("y0")?,
        get("x1")?,
        get("y1")?,
        get("scale")?,
    );
    if z > 18 {
        return Err("z must be 0 to 18".into());
    }
    let n = 1u64 << z;
    if x0 > x1 || y0 > y1 || x1 >= n || y1 >= n {
        return Err("rectangle outside the zoom level".into());
    }
    if (x1 - x0 + 1) * (y1 - y0 + 1) > MAX_TILES {
        return Err(format!("at most {MAX_TILES} tiles per request"));
    }
    if !(1..=4).contains(&scale) {
        return Err("scale must be 1 to 4".into());
    }
    let view = engine.view.read().expect("view lock").clone();
    let (cx, cy) = ((x0 + x1) as f64 / 2.0, (y0 + y1) as f64 / 2.0);
    let mut keys: Vec<TileKey> = (y0..=y1)
        .flat_map(|y| (x0..=x1).map(move |x| (x, y)))
        .map(|(x, y)| TileKey {
            z: z as u32,
            x: x as u32,
            y: y as u32,
            scale: scale as u32,
        })
        .collect();
    keys.sort_by(|a, b| {
        let d = |k: &TileKey| (f64::from(k.x) - cx).powi(2) + (f64::from(k.y) - cy).powi(2);
        d(a).total_cmp(&d(b))
    });
    let mut q = engine.queue.lock().expect("queue lock");
    q.retain(|j| j.client != id);
    for key in keys {
        let rel = tile_path(&key);
        if view.tiles.join(&rel).exists() {
            drop(q);
            engine.send(id, tile_message(&key, Some(&rel), None));
            q = engine.queue.lock().expect("queue lock");
        } else {
            q.push_back(Job {
                client: id,
                key,
                generation: view.generation.clone(),
            });
        }
    }
    drop(q);
    engine.ready.notify_all();
    Ok(())
}

fn number(m: &Value, key: &str) -> Result<f64, String> {
    m.get(key)
        .and_then(Value::as_f64)
        .filter(|v| v.is_finite())
        .ok_or_else(|| format!("{key} must be a number"))
}

fn query(engine: &Arc<Engine>, m: &Value) -> Result<Value, String> {
    let lat = number(m, "lat")?;
    let lon = number(m, "lon")?;
    let zoom = number(m, "zoom")?.clamp(0.0, 20.0);
    if !(-90.0..=90.0).contains(&lat) || !(-180.0..=180.0).contains(&lon) {
        return Err("position out of range".into());
    }
    let view = engine.view.read().expect("view lock").clone();
    let features = features_at(&view.library, &view.style.settings, lat, lon, zoom);
    let mut reply =
        json!({"type": "features", "v": VERSION, "lat": lat, "lon": lon, "features": features});
    if let Some(id) = m.get("id") {
        reply["id"] = id.clone();
    }
    Ok(reply)
}

/// What's charted at a point: the finest chart's features near it first,
/// then points, lines and areas in that order.
pub fn features_at(lib: &Library, set: &Settings, lat: f64, lon: f64, zoom: f64) -> Vec<Value> {
    let p = mercator(lon, lat);
    let unit = 1.0 / (zoom.exp2() * TILE);
    let tol = 10.0 * unit;
    let display = geo::scale_denominator(zoom, lat);
    let near = Rect {
        x0: p[0],
        y0: p[1],
        x1: p[0],
        y1: p[1],
    }
    .expand(tol);
    let mut cells = render::cells_for(lib, &near, display);
    cells.reverse();
    let mut found: Vec<(u8, f64, Value)> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for entry in cells {
        let Ok(chart) = lib.load(entry) else { continue };
        let covered = chart.coverage.is_empty() || geo::inside(&chart.coverage, p);
        for item in &chart.items {
            // A light on a buoy or beacon is told with it.
            let told = item.class == LIGHTS && item.master.is_some();
            if skip(item.class) || told || !item.bbox.intersects(&near) || !seen.insert(item.id) {
                continue;
            }
            let hit = match &item.geom {
                Geom::Point(q) => dist(p, *q).filter(|d| *d <= tol).map(|d| (0, d)),
                Geom::Soundings(list) => list
                    .iter()
                    .map(|(q, _)| dist(p, *q).unwrap_or(f64::MAX))
                    .fold(None, |best: Option<f64>, d| {
                        Some(best.map_or(d, |b| b.min(d)))
                    })
                    .filter(|d| *d <= tol)
                    .map(|d| (3, d)),
                Geom::Lines(lines) => line_distance(p, lines)
                    .filter(|d| *d <= tol)
                    .map(|d| (1, d)),
                Geom::Area { rings, .. } => (covered && geo::inside(rings, p)).then_some((2, 0.0)),
                Geom::None => None,
            };
            if let Some((rank, d)) = hit {
                found.push((rank, d, describe(&chart, item, p, set)));
            }
        }
        // The finest chart that covers the point answers for its areas.
        if covered && found.iter().any(|(r, _, _)| *r == 2) {
            break;
        }
    }
    found.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)));
    // One area is often charted as several pieces with the same name.
    let mut told = std::collections::HashSet::new();
    found
        .into_iter()
        .map(|(_, _, v)| v)
        .filter(|v| told.insert((v["class"].to_string(), v["title"].to_string())))
        .take(12)
        .collect()
}

fn dist(a: [f64; 2], b: [f64; 2]) -> Option<f64> {
    Some(((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt())
}

fn line_distance(p: [f64; 2], lines: &[Vec<[f64; 2]>]) -> Option<f64> {
    lines
        .iter()
        .flat_map(|l| l.windows(2))
        .map(|w| geo::segment_distance(p, w[0], w[1]))
        .fold(None, |best: Option<f64>, d| {
            Some(best.map_or(d, |b| b.min(d)))
        })
}

/// Metadata and collections: nothing a sailor clicks on.
fn skip(class: u16) -> bool {
    let a = s57::acronym(class);
    a.starts_with("M_") || a.starts_with("C_") || matches!(class, DAYMAR | TOPMAR) || a == "?"
}

fn describe(chart: &Chart, item: &Item, at: [f64; 2], set: &Settings) -> Value {
    let u = set.units;
    let depth = |m: f64| marks::depth_words(m, u);
    let kind = s57::class_name(item.class);
    let name = item.attr(OBJNAM).or(item.attr(NOBJNM)).map(String::from);
    let mut lines: Vec<String> = Vec::new();
    let mut label = None;
    let mut title = name.clone();
    let colours = || -> Option<String> {
        let c: Vec<&str> = item
            .list(COLOUR)
            .iter()
            .map(|&c| marks::colour_name(c))
            .collect();
        (!c.is_empty()).then(|| capitalise(&c.join(", ")))
    };
    match item.class {
        BOYLAT | BOYCAR | BOYISD | BOYSAW | BOYSPP | BOYINB | BCNLAT | BCNCAR | BCNISD | BCNSAW
        | BCNSPP | LITFLT | LITVES => {
            label = Some(marks::aid_label(item));
            lines.extend(colours());
            if let Some(s) = item
                .first(BOYSHP)
                .map(marks::buoy_shape)
                .filter(|s| !s.is_empty())
            {
                lines.push(capitalise(s));
            }
            for &j in &item.slaves {
                let l = &chart.items[j];
                if l.class == LIGHTS {
                    lines.push(light_line(l, set));
                }
            }
        }
        LIGHTS => {
            lines.push(light_line(item, set));
            if let (Some(a), Some(b)) = (item.num(SECTR1), item.num(SECTR2)) {
                lines.push(format!(
                    "Sector {}° to {}°, from seaward",
                    marks::trim_number(a),
                    marks::trim_number(b)
                ));
            }
        }
        DEPARE | DRGARE => {
            let d1 = item.num(DRVAL1);
            let d2 = item.num(DRVAL2);
            let range = match (d1, d2) {
                (Some(a), Some(b)) if a < 0.0 && b <= 0.0 => "Dries".to_string(),
                (Some(a), Some(b)) => {
                    format!("{} to {}", marks::depth_number(a.max(0.0), u), depth(b))
                }
                (Some(a), None) => format!("{} or more", depth(a)),
                _ => "Depth unknown".to_string(),
            };
            if item.class == DRGARE {
                lines.push(format!(
                    "Dredged to {}",
                    d1.map_or("an unknown depth".into(), depth)
                ));
            }
            title = Some(title.map_or(range.clone(), |t| format!("{t}: {range}")));
        }
        SOUNDG => {
            if let Geom::Soundings(list) = &item.geom
                && let Some((_, d)) = list.iter().min_by(|a, b| {
                    let da = (a.0[0] - at[0]).powi(2) + (a.0[1] - at[1]).powi(2);
                    let db = (b.0[0] - at[0]).powi(2) + (b.0[1] - at[1]).powi(2);
                    da.total_cmp(&db)
                })
            {
                title = Some(format!("Sounding {}", depth(f64::from(*d))));
            }
        }
        DEPCNT => {
            if let Some(v) = item.num(VALDCO) {
                title = Some(format!("{} contour", depth(v)));
            }
        }
        UWTROC | WRECKS | OBSTRN => {
            if let Some(d) = item.num(VALSOU) {
                lines.push(format!("Least depth {}", depth(d)));
            }
            if let Some(w) = item
                .first(WATLEV)
                .map(marks::water_level)
                .filter(|w| !w.is_empty())
            {
                lines.push(capitalise(w));
            }
        }
        BRIDGE | CBLOHD | PIPOHD => {
            for (code, what) in [
                (VERCLR, "Vertical clearance"),
                (VERCCL, "Clearance closed"),
                (VERCOP, "Clearance open"),
            ] {
                if let Some(v) = item.num(code) {
                    lines.push(format!("{what} {}", marks::depth_words(v, u)));
                }
            }
            if let Some(h) = item.num(HORCLR) {
                lines.push(format!("Horizontal clearance {}", marks::depth_words(h, u)));
            }
        }
        SBDARE => {
            let b = marks::bottom(item);
            if !b.is_empty() {
                title = Some(format!("Bottom: {b}"));
            }
        }
        _ => {}
    }
    if let Some(info) = item.attr(INFORM).or(item.attr(NINFOM)) {
        lines.push(info.to_string());
    }
    let mut v = json!({
        "class": s57::acronym(item.class),
        "kind": kind,
        "title": title.unwrap_or_else(|| kind.to_string()),
        "lines": lines,
        "chart": chart.name,
    });
    if let Some(l) = label.filter(|l| !l.is_empty()) {
        v["label"] = json!(l);
    }
    v
}

fn light_line(l: &Item, set: &Settings) -> String {
    let mut s = format!("Light: {}", marks::light(l));
    if let Some(h) = l.num(HEIGHT) {
        s.push_str(&format!(", {} high", marks::depth_words(h, set.units)));
    }
    s
}

fn capitalise(s: &str) -> String {
    let mut c = s.chars();
    c.next()
        .map_or_else(String::new, |f| f.to_uppercase().chain(c).collect())
}

/// The charts directory the engine and the CLI agree on.
pub fn charts_root() -> PathBuf {
    library::default_root()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tile_paths_and_messages() {
        let key = TileKey {
            z: 15,
            x: 5249,
            y: 12655,
            scale: 2,
        };
        assert_eq!(tile_path(&key), "15/5249/12655@2.png");
        let m: Value = serde_json::from_str(&tile_message(&key, Some("p"), None)).unwrap();
        assert_eq!(m["type"], "tile");
        assert_eq!(m["path"], "p");
        assert!(m.get("error").is_none());
    }

    #[test]
    fn prune_keeps_named_generations_only() {
        let dir = std::env::temp_dir().join(format!("omahelm-prune-{}", std::process::id()));
        for g in ["0123456789abcdef", "fedcba9876543210", "notageneration"] {
            std::fs::create_dir_all(dir.join(g)).unwrap();
        }
        prune(&dir, &["0123456789abcdef"]);
        assert!(dir.join("0123456789abcdef").exists());
        assert!(!dir.join("fedcba9876543210").exists());
        assert!(dir.join("notageneration").exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
