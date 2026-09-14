//! The charts on disk: an index of every cell's scale and coverage, and a
//! cache of the cells lately drawn.
//!
//! Charts live under `$XDG_DATA_HOME/omahelm/charts/ENC_ROOT/<CELL>/`, as
//! NOAA ships them. `index.json` beside `ENC_ROOT` lists them; it's
//! rebuilt whenever a cell file changes.

use crate::chart::{Chart, P};
use crate::geo::Rect;
use crate::s57::Cell;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::UNIX_EPOCH;

/// Bump when the index format changes.
const INDEX_VERSION: u32 = 1;
/// Cells kept in memory. A Bay tile at harbor scale needs about ten.
const CACHE: usize = 48;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Entry {
    pub name: String,
    /// Relative to the charts directory.
    pub path: PathBuf,
    pub scale: u32,
    pub edition: u32,
    pub update: u32,
    pub issued: String,
    /// x0, y0, x1, y1 in Web Mercator.
    pub bounds: [f64; 4],
    pub coverage: Vec<Vec<P>>,
    /// Size and mtime of the cell and its updates, to notice a change.
    pub stamp: String,
}

impl Entry {
    pub fn rect(&self) -> Rect {
        let [x0, y0, x1, y1] = self.bounds;
        Rect { x0, y0, x1, y1 }
    }
}

#[derive(Serialize, Deserialize)]
struct Index {
    version: u32,
    cells: Vec<Entry>,
    /// Cells that couldn't be read, and why.
    #[serde(default)]
    problems: Vec<(String, String)>,
}

pub struct Library {
    pub root: PathBuf,
    pub entries: Vec<Entry>,
    pub problems: Vec<(String, String)>,
    cache: Mutex<Vec<(String, Arc<Chart>)>>,
}

pub fn default_root() -> PathBuf {
    if let Ok(p) = std::env::var("OMAHELM_CHARTS") {
        return PathBuf::from(p);
    }
    let base = std::env::var("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| crate::style::home().join(".local/share"));
    base.join("omahelm/charts")
}

/// Every base cell (`*.000`) under a directory.
pub fn find_cells(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else {
            continue;
        };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x == "000") {
                out.push(p);
            }
        }
    }
    out.sort();
    out
}

fn stamp(base: &Path) -> String {
    let mut parts = Vec::new();
    for p in std::iter::once(base.to_path_buf()).chain(crate::s57::updates(base)) {
        if let Ok(m) = std::fs::metadata(&p) {
            let t = m
                .modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_secs());
            parts.push(format!("{}:{t}", m.len()));
        }
    }
    parts.join(",")
}

impl Library {
    /// No charts yet, while the index is read.
    pub fn empty(root: &Path) -> Library {
        Library {
            root: root.to_path_buf(),
            entries: Vec::new(),
            problems: Vec::new(),
            cache: Mutex::new(Vec::new()),
        }
    }

    /// Opens the charts directory, re-indexing any cell that changed.
    /// `progress` hears (done, total) while cells are read.
    pub fn open(root: &Path, progress: &(dyn Fn(usize, usize) + Sync)) -> Library {
        let index_path = root.join("index.json");
        let old: Option<Index> = std::fs::read(&index_path)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .filter(|i: &Index| i.version == INDEX_VERSION);
        let (old_cells, old_problems) =
            old.map_or((Vec::new(), Vec::new()), |i| (i.cells, i.problems));
        let bases = find_cells(root);
        let mut entries = Vec::new();
        let mut problems = Vec::new();
        let mut todo = Vec::new();
        for base in &bases {
            let rel = base.strip_prefix(root).unwrap_or(base).to_path_buf();
            let st = stamp(base);
            if let Some(e) = old_cells.iter().find(|e| e.path == rel && e.stamp == st) {
                entries.push(e.clone());
            } else if let Some(p) = old_problems
                .iter()
                .find(|(p, _)| *p == format!("{}|{st}", rel.display()))
            {
                problems.push(p.clone());
            } else {
                todo.push((base.clone(), rel, st));
            }
        }
        let changed = !todo.is_empty()
            || entries.len() + problems.len() != old_cells.len() + old_problems.len();
        if !todo.is_empty() {
            let done = std::sync::atomic::AtomicUsize::new(0);
            let threads = std::thread::available_parallelism()
                .map_or(2, |n| n.get())
                .min(8);
            let total = todo.len();
            let chunk = total.div_ceil(threads);
            let todo = &todo;
            let results: Vec<Result<Entry, (String, String)>> = std::thread::scope(|s| {
                let handles: Vec<_> = todo
                    .chunks(chunk)
                    .map(|part| {
                        let done = &done;
                        s.spawn(move || {
                            part.iter()
                                .map(|(base, rel, st)| {
                                    let r = read_entry(base, rel, st)
                                        .map_err(|e| (format!("{}|{st}", rel.display()), e));
                                    let n =
                                        done.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
                                    progress(n, total);
                                    r
                                })
                                .collect::<Vec<_>>()
                        })
                    })
                    .collect();
                handles
                    .into_iter()
                    .flat_map(|h| h.join().unwrap_or_default())
                    .collect()
            });
            for r in results {
                match r {
                    Ok(e) => entries.push(e),
                    Err(p) => problems.push(p),
                }
            }
        }
        entries.sort_by(|a, b| a.name.cmp(&b.name));
        if changed {
            let index = Index {
                version: INDEX_VERSION,
                cells: entries.clone(),
                problems: problems.clone(),
            };
            if let Ok(json) = serde_json::to_vec(&index) {
                let tmp = index_path.with_extension("json.tmp");
                if std::fs::write(&tmp, json).is_ok() {
                    let _ = std::fs::rename(&tmp, &index_path);
                }
            }
        }
        Library {
            root: root.to_path_buf(),
            entries,
            problems,
            cache: Mutex::new(Vec::new()),
        }
    }

    /// Cells whose bounds meet a rectangle.
    pub fn around(&self, rect: &Rect) -> Vec<&Entry> {
        self.entries
            .iter()
            .filter(|e| e.rect().intersects(rect))
            .collect()
    }

    /// A cell, from the cache or read from disk.
    pub fn load(&self, entry: &Entry) -> Result<Arc<Chart>, String> {
        {
            let mut cache = self.cache.lock().expect("chart cache lock");
            if let Some(i) = cache.iter().position(|(n, _)| *n == entry.name) {
                let hit = cache.remove(i);
                let chart = hit.1.clone();
                cache.push(hit);
                return Ok(chart);
            }
        }
        let chart = Arc::new(Chart::from_cell(Cell::open(&self.root.join(&entry.path))?));
        let mut cache = self.cache.lock().expect("chart cache lock");
        if !cache.iter().any(|(n, _)| *n == entry.name) {
            cache.push((entry.name.clone(), chart.clone()));
            if cache.len() > CACHE {
                cache.remove(0);
            }
        }
        Ok(chart)
    }

    /// Drops every cached cell, to give the memory back while idle.
    pub fn forget(&self) {
        self.cache.lock().expect("chart cache lock").clear();
    }

    /// Changes whenever any cell, edition or update does.
    pub fn fingerprint(&self) -> String {
        let mut h = Fnv::default();
        for e in &self.entries {
            h.write(e.name.as_bytes());
            h.write(&e.edition.to_le_bytes());
            h.write(&e.update.to_le_bytes());
            h.write(e.stamp.as_bytes());
        }
        format!("{:016x}", h.0)
    }

    /// The bounds of every chart, for "no charts here" and the first view.
    pub fn extent(&self) -> Rect {
        let mut r = Rect::EMPTY;
        for e in &self.entries {
            r.union(&e.rect());
        }
        r
    }
}

fn read_entry(base: &Path, rel: &Path, st: &str) -> Result<Entry, String> {
    let chart = Chart::from_cell(Cell::open(base)?);
    if chart.bounds.is_empty() {
        return Err("no coverage".into());
    }
    let b = chart.bounds;
    Ok(Entry {
        name: chart.name.clone(),
        path: rel.to_path_buf(),
        scale: chart.scale,
        edition: chart.edition,
        update: chart.update,
        issued: chart.issued.clone(),
        bounds: [b.x0, b.y0, b.x1, b.y1],
        coverage: chart.coverage.clone(),
        stamp: st.to_string(),
    })
}

/// FNV-1a, for short stable fingerprints.
pub struct Fnv(pub u64);

impl Default for Fnv {
    fn default() -> Self {
        Fnv(0xcbf2_9ce4_8422_2325)
    }
}

impl Fnv {
    pub fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.0 ^= u64::from(b);
            self.0 = self.0.wrapping_mul(0x0100_0000_01b3);
        }
    }
}
