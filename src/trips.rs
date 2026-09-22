//! The logbook's tracks on the chart: every passage omalogbook wrote,
//! gathered a day at a time into the single trip it was.
//!
//! omalogbook closes a passage whenever the fix is lost for long enough, so
//! one afternoon's sail lands on disk as several GPX files with holes
//! between them. Here they are put back together: a day's passages in the
//! order they were sailed, and a straight line across each hole.
//!
//! A straight line is not where the boat went. It is only the shortest
//! thing it could have done, and at the Gate on a flood it is nowhere near
//! the truth. So the two are kept apart all the way to the screen: a `run`
//! is recorded, a `gap` is inferred, they are counted separately, and the
//! chart draws them differently.
//!
//! Nothing here writes to the vault. omalogbook owns it; this only reads.

use crate::geo::{self, Rect};
use crate::library::Fnv;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

/// A hole this long is a gap rather than a stretch of track. omalogbook
/// writes a point every ten seconds by default and a receiver misses one
/// now and then, so the threshold is well clear of that.
const GAP_SECONDS: i64 = 90;
/// Points per day after thinning. A day at the far end of a season's view
/// is a few strokes; this only stops one pathological file from filling
/// the socket.
const MAX_POINTS: usize = 4000;
/// Days one request may ask for.
pub const MAX_DAYS: usize = 500;

/// A fix as the log wrote it. `epoch` is 0 for a point with no time, which
/// GPX allows and some editors write.
#[derive(Clone, Copy, Debug)]
pub struct Point {
    pub epoch: i64,
    pub lat: f64,
    pub lon: f64,
}

/// Two fixes are the same point when they are in the same place; the
/// clock isn't part of where the boat was.
impl PartialEq for Point {
    fn eq(&self, other: &Point) -> bool {
        self.lat == other.lat && self.lon == other.lon
    }
}

/// A stretch of a day: `Run` is where the boat was seen, `Gap` the straight
/// line across the hole between two runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Run,
    Gap,
}

#[derive(Clone, Debug)]
pub struct Leg {
    pub kind: Kind,
    pub points: Vec<Point>,
}

impl Leg {
    fn nm(&self) -> f64 {
        self.points
            .windows(2)
            .map(|w| geo::distance_nm((w[0].lat, w[0].lon), (w[1].lat, w[1].lon)))
            .sum()
    }
}

/// One day's sailing, as one trip.
#[derive(Clone, Debug)]
pub struct Day {
    /// The local date the passages were named for, `YYYY-MM-DD`.
    pub date: String,
    pub passages: usize,
    pub legs: Vec<Leg>,
}

impl Day {
    pub fn runs(&self) -> impl Iterator<Item = &Leg> {
        self.legs.iter().filter(|l| l.kind == Kind::Run)
    }

    pub fn gaps(&self) -> impl Iterator<Item = &Leg> {
        self.legs.iter().filter(|l| l.kind == Kind::Gap)
    }

    /// Points actually recorded, gap ends not counted twice.
    pub fn points(&self) -> usize {
        self.runs().map(|l| l.points.len()).sum()
    }

    pub fn run_nm(&self) -> f64 {
        self.runs().map(Leg::nm).sum()
    }

    pub fn gap_nm(&self) -> f64 {
        self.gaps().map(Leg::nm).sum()
    }

    pub fn span(&self) -> (i64, i64) {
        self.times()
    }

    fn times(&self) -> (i64, i64) {
        let stamps = || {
            self.runs()
                .flat_map(|l| l.points.iter())
                .map(|p| p.epoch)
                .filter(|e| *e > 0)
        };
        (
            stamps().min().unwrap_or_default(),
            stamps().max().unwrap_or_default(),
        )
    }

    /// West, south, east and north of everything drawn, gaps included.
    fn bounds(&self) -> Option<[f64; 4]> {
        let mut b = [f64::MAX, f64::MAX, f64::MIN, f64::MIN];
        let mut any = false;
        for p in self.legs.iter().flat_map(|l| l.points.iter()) {
            any = true;
            b[0] = b[0].min(p.lon);
            b[1] = b[1].min(p.lat);
            b[2] = b[2].max(p.lon);
            b[3] = b[3].max(p.lat);
        }
        any.then_some(b)
    }

    fn bbox(&self) -> Option<Value> {
        self.bounds()
            .map(|b| json!({"west": b[0], "south": b[1], "east": b[2], "north": b[3]}))
    }

    /// The day as the calendar and the status line read it.
    pub fn summary(&self) -> Value {
        let (from, to) = self.times();
        let mut v = json!({
            "date": self.date,
            "passages": self.passages,
            "points": self.points(),
            "distanceNm": round(self.run_nm() + self.gap_nm(), 2),
            "gapNm": round(self.gap_nm(), 2),
            "gaps": self.gaps().count(),
        });
        if from > 0 {
            v["from"] = json!(iso_utc(from));
            v["to"] = json!(iso_utc(to));
            v["seconds"] = json!(to - from);
        }
        if let Some(b) = self.bbox() {
            v["bbox"] = b;
        }
        v
    }

    /// The day as the chart draws it: runs and gaps as separate lines of
    /// `lat, lon, lat, lon…`, thinned for a zoom level.
    pub fn drawing(&self, z: u32) -> Value {
        let mut tol = tolerance(z);
        let (mut runs, mut gaps) = self.lines(tol);
        // A day that still won't fit is thinned harder rather than cut off
        // halfway, which would draw a trip that stops in open water.
        while count(&runs) > MAX_POINTS && tol < 1.0 {
            tol *= 4.0;
            let next = self.lines(tol);
            runs = next.0;
            gaps = next.1;
        }
        let mut v = self.summary();
        v["z"] = json!(z);
        v["drawn"] = json!(count(&runs));
        v["runs"] = json!(runs);
        v["gaps"] = json!(gaps);
        v
    }

    fn lines(&self, tol: f64) -> (Vec<Vec<f64>>, Vec<Vec<f64>>) {
        let mut runs = Vec::new();
        let mut gaps = Vec::new();
        for leg in &self.legs {
            // A hole the boat sat still through — the log closed and
            // opened again at the slip — has nothing to draw, and a line
            // of no length would leave a dot nobody can read.
            if leg.kind == Kind::Gap && leg.points.first() == leg.points.last() {
                continue;
            }
            let kept = simplify(&leg.points, tol);
            if kept.len() < 2 {
                continue;
            }
            let mut flat = Vec::with_capacity(kept.len() * 2);
            for i in kept {
                flat.push(round(leg.points[i].lat, 6));
                flat.push(round(leg.points[i].lon, 6));
            }
            if leg.kind == Kind::Run {
                runs.push(flat);
            } else {
                gaps.push(flat);
            }
        }
        (runs, gaps)
    }

    /// The whole day as one GPX track, the gaps closed: `omahelm trip`.
    /// One `<trkseg>`, because the point of the file is that it is a
    /// single trip; what was inferred is said in the description.
    pub fn gpx(&self, boat: &str) -> String {
        let gaps = self.gaps().count();
        let mut out = String::from(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
             <gpx version=\"1.1\" creator=\"omahelm\" xmlns=\"http://www.topografix.com/GPX/1/1\">\n",
        );
        out.push_str("  <trk>\n");
        out.push_str(&format!(
            "    <name>{} {}</name>\n",
            escape(boat),
            self.date
        ));
        out.push_str(&format!(
            "    <desc>{} passages joined, {} gap{} filled with straight lines ({} of {} nm inferred). Not where the boat went.</desc>\n",
            self.passages,
            gaps,
            if gaps == 1 { "" } else { "s" },
            format_nm(self.gap_nm()),
            format_nm(self.run_nm() + self.gap_nm()),
        ));
        out.push_str("    <trkseg>\n");
        let mut last: Option<Point> = None;
        for leg in &self.legs {
            for p in &leg.points {
                // A gap's ends are the runs' own points, already written.
                if last.is_some_and(|l| l == *p) {
                    continue;
                }
                out.push_str(&format!(
                    "      <trkpt lat=\"{:.6}\" lon=\"{:.6}\">\n",
                    p.lat, p.lon
                ));
                if p.epoch > 0 {
                    out.push_str(&format!("        <time>{}</time>\n", iso_utc(p.epoch)));
                }
                out.push_str("      </trkpt>\n");
                last = Some(*p);
            }
        }
        out.push_str("    </trkseg>\n  </trk>\n</gpx>\n");
        out
    }
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

pub fn format_nm(nm: f64) -> String {
    format!("{nm:.1}")
}

/// Points in a set of lines of `lat, lon, lat, lon…`.
fn count(lines: &[Vec<f64>]) -> usize {
    lines.iter().map(Vec::len).sum::<usize>() / 2
}

fn round(v: f64, places: i32) -> f64 {
    let f = 10f64.powi(places);
    (v * f).round() / f
}

/// How far a point may be from the line between its neighbors and still be
/// dropped: a third of a logical pixel at that zoom, in Mercator units.
fn tolerance(z: u32) -> f64 {
    1.0 / (256.0 * f64::from(z.min(22)).exp2() * 3.0)
}

/// Douglas–Peucker, in the projection the chart draws in, returning the
/// indices kept. Iterative: a day can be thousands of points deep.
fn simplify(points: &[Point], tol: f64) -> Vec<usize> {
    if points.len() <= 2 {
        return (0..points.len()).collect();
    }
    let xy: Vec<[f64; 2]> = points.iter().map(|p| geo::mercator(p.lon, p.lat)).collect();
    let mut keep = vec![false; points.len()];
    keep[0] = true;
    keep[points.len() - 1] = true;
    let mut stack = vec![(0usize, points.len() - 1)];
    while let Some((a, b)) = stack.pop() {
        if b <= a + 1 {
            continue;
        }
        let mut worst = 0.0;
        let mut at = a;
        for (i, p) in xy.iter().enumerate().take(b).skip(a + 1) {
            let d = geo::segment_distance(*p, xy[a], xy[b]);
            if d > worst {
                worst = d;
                at = i;
            }
        }
        if worst > tol {
            keep[at] = true;
            stack.push((a, at));
            stack.push((at, b));
        }
    }
    (0..points.len()).filter(|i| keep[*i]).collect()
}

// ------------------------------------------------------------------ files

/// Points in one GPX file, a list per `<trkseg>`: a segment is a
/// continuous stretch of track, which is exactly what a run is.
pub fn parse_gpx(text: &str) -> Vec<Vec<Point>> {
    let mut segments = Vec::new();
    for chunk in text.split("<trkseg").skip(1) {
        let body = chunk.split("</trkseg>").next().unwrap_or(chunk);
        let mut points: Vec<Point> = Vec::new();
        for piece in body.split("<trkpt").skip(1) {
            let Some((tag, rest)) = piece.split_once('>') else {
                continue;
            };
            let (Some(lat), Some(lon)) = (attr(tag, "lat"), attr(tag, "lon")) else {
                continue;
            };
            if !(-90.0..=90.0).contains(&lat) || !(-180.0..=180.0).contains(&lon) {
                continue;
            }
            // An empty element carries nothing else; otherwise the point
            // ends at its closing tag.
            let epoch = if tag.trim_end().ends_with('/') {
                None
            } else {
                let inner = rest.split("</trkpt>").next().unwrap_or(rest);
                between(inner, "<time>", "</time>").and_then(epoch_from_iso)
            };
            points.push(Point {
                epoch: epoch.unwrap_or(0),
                lat,
                lon,
            });
        }
        if !points.is_empty() {
            segments.push(points);
        }
    }
    segments
}

fn between<'a>(text: &'a str, open: &str, close: &str) -> Option<&'a str> {
    let start = text.find(open)? + open.len();
    let rest = &text[start..];
    Some(&rest[..rest.find(close)?])
}

/// A numeric attribute of a start tag, quoted either way.
fn attr(tag: &str, name: &str) -> Option<f64> {
    for quote in ['"', '\''] {
        let key = format!("{name}={quote}");
        let mut from = 0;
        while let Some(i) = tag[from..].find(&key) {
            let at = from + i;
            let before = tag[..at].chars().next_back();
            // `lat=` and not the tail of another name.
            if before.is_none_or(char::is_whitespace) {
                let value = &tag[at + key.len()..];
                return value
                    .split(quote)
                    .next()
                    .and_then(|v| v.trim().parse::<f64>().ok())
                    .filter(|v| v.is_finite());
            }
            from = at + key.len();
        }
    }
    None
}

/// `2026-09-21T18:43:30Z`, with or without a fraction, Z or an offset.
pub fn epoch_from_iso(s: &str) -> Option<i64> {
    let s = s.trim();
    let b = s.as_bytes();
    if b.len() < 19 || b[4] != b'-' || b[7] != b'-' || b[13] != b':' || b[16] != b':' {
        return None;
    }
    if b[10] != b'T' && b[10] != b't' && b[10] != b' ' {
        return None;
    }
    let num = |at: usize, n: usize| s.get(at..at + n)?.parse::<i64>().ok();
    let (year, month, day) = (num(0, 4)?, num(5, 2)?, num(8, 2)?);
    let (hour, minute, second) = (num(11, 2)?, num(14, 2)?, num(17, 2)?);
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    if hour > 23 || minute > 59 || second > 60 {
        return None;
    }
    let mut epoch = days_from_civil(year, month, day) * 86_400 + hour * 3600 + minute * 60 + second;
    let zone = s[19..].trim_start_matches(|c: char| c == '.' || c.is_ascii_digit());
    let zone = zone.trim();
    if zone.is_empty() || zone.eq_ignore_ascii_case("z") {
        return Some(epoch);
    }
    let sign = match zone.as_bytes()[0] {
        b'+' => 1,
        b'-' => -1,
        _ => return None,
    };
    let digits: String = zone[1..].chars().filter(char::is_ascii_digit).collect();
    if digits.len() != 4 {
        return None;
    }
    let offset = digits[..2].parse::<i64>().ok()? * 3600 + digits[2..].parse::<i64>().ok()? * 60;
    epoch -= sign * offset;
    Some(epoch)
}

/// Days from 1970-01-01, by Howard Hinnant's civil_from_days, run backwards.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = year - i64::from(month <= 2);
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (y + i64::from(m <= 2), m, d)
}

pub fn iso_utc(epoch: i64) -> String {
    let (y, m, d) = civil_from_days(epoch.div_euclid(86_400));
    let s = epoch.rem_euclid(86_400);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        s / 3600,
        (s / 60) % 60,
        s % 60
    )
}

/// A moment in the machine's own zone, or nothing when it can't be read.
fn local(epoch: i64) -> Option<libc::tm> {
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    let t = epoch as libc::time_t;
    // SAFETY: localtime_r fills the caller's tm and touches nothing else.
    let filled = unsafe { libc::localtime_r(&t, &mut tm) };
    (!filled.is_null()).then_some(tm)
}

/// The local date of a moment, `YYYY-MM-DD`: the day a sailor would file
/// it under, which is the day omalogbook names its files for.
pub fn local_date(epoch: i64) -> String {
    local(epoch).map_or_else(
        || iso_utc(epoch)[..10].to_string(),
        |tm| {
            format!(
                "{:04}-{:02}-{:02}",
                tm.tm_year + 1900,
                tm.tm_mon + 1,
                tm.tm_mday
            )
        },
    )
}

/// The local clock of a moment, `HH:MM`, as the log writes it.
pub fn local_clock(epoch: i64) -> String {
    local(epoch).map_or_else(
        || iso_utc(epoch)[11..16].to_string(),
        |tm| format!("{:02}:{:02}", tm.tm_hour, tm.tm_min),
    )
}

/// `2026-09-21` from `2026-09-21-114330.gpx`, the way omalogbook names a
/// passage: for the local day it began.
fn date_from_name(name: &str) -> Option<String> {
    let head = name.get(..10)?;
    let b = head.as_bytes();
    if b[4] != b'-' || b[7] != b'-' {
        return None;
    }
    let num = |at: usize, n: usize| head.get(at..at + n)?.parse::<u32>().ok();
    let (year, month, day) = (num(0, 4)?, num(5, 2)?, num(8, 2)?);
    ((1..=12).contains(&month) && (1..=31).contains(&day) && year >= 1000).then(|| head.to_string())
}

pub fn is_date(s: &str) -> bool {
    s.len() == 10 && date_from_name(s).is_some()
}

// ------------------------------------------------------------- the vault

/// Where the logbook is: omahelm's own `logbook` setting, else the vault
/// omalogbook is configured for, else `~/Logbook`, which is its default.
pub fn default_vault(configured: Option<&str>) -> PathBuf {
    if let Some(p) = configured.filter(|p| !p.is_empty()) {
        return expand(p);
    }
    omalogbook_setting("vault").map_or_else(|| crate::style::home().join("Logbook"), |v| expand(&v))
}

/// The boat whose log this is, for an exported track's name.
pub fn boat_name() -> String {
    omalogbook_setting("boat").unwrap_or_else(|| "Boat".into())
}

/// One key out of a note's YAML front matter: the block between the first
/// `---` and the next. Only the plain `key: value` lines omalogbook
/// writes; anything else is the crew's and is left alone.
fn front_matter(text: &str, key: &str) -> Option<String> {
    let rest = text
        .strip_prefix("---\n")
        .or_else(|| text.strip_prefix("---\r\n"))?;
    for line in rest.lines() {
        if line.trim_end() == "---" {
            break;
        }
        if let Some((k, v)) = line.split_once(':')
            && k.trim() == key
        {
            let v = v.trim().trim_matches('"').trim_matches('\'').trim();
            if !v.is_empty() {
                return Some(v.to_string());
            }
        }
    }
    None
}

/// One setting out of omalogbook's config, which is the same flat
/// `key = "value"` shape omahelm's own is.
fn omalogbook_setting(key: &str) -> Option<String> {
    let text = std::fs::read_to_string(omalogbook_config()).ok()?;
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        if let Some((k, v)) = line.split_once('=')
            && k.trim() == key
        {
            let v = v.trim().trim_matches('"').trim();
            if !v.is_empty() {
                return Some(v.to_string());
            }
        }
    }
    None
}

fn omalogbook_config() -> PathBuf {
    let base = std::env::var("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| crate::style::home().join(".config"));
    base.join("omalogbook/config.toml")
}

pub fn expand(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/") {
        crate::style::home().join(rest)
    } else if path == "~" {
        crate::style::home()
    } else {
        PathBuf::from(path)
    }
}

/// One GPX file, filed under the day it belongs to.
struct Filed {
    date: String,
    /// Its first timed fix, which orders the day's passages.
    start: i64,
    segments: Arc<Vec<Vec<Point>>>,
}

#[derive(Clone)]
struct Entry {
    len: u64,
    modified: Option<SystemTime>,
    segments: Arc<Vec<Vec<Point>>>,
}

/// Every day the logbook has a track for, kept in memory and re-read when
/// the files change.
pub struct Log {
    vault: PathBuf,
    tracks: PathBuf,
    files: HashMap<PathBuf, Entry>,
    days: Vec<Day>,
    stamp: String,
    /// Files that held no usable point.
    pub skipped: usize,
}

impl Log {
    pub fn open(vault: PathBuf) -> Log {
        // `tracks/` is where omalogbook keeps them; a folder of GPX files
        // named straight at the setting works too.
        let tracks = if vault.join("tracks").is_dir() {
            vault.join("tracks")
        } else {
            vault.clone()
        };
        let mut log = Log {
            vault,
            tracks,
            files: HashMap::new(),
            days: Vec::new(),
            stamp: String::new(),
            skipped: 0,
        };
        log.refresh();
        log
    }

    pub fn vault(&self) -> &Path {
        &self.vault
    }

    pub fn status(&self) -> &'static str {
        if !self.tracks.is_dir() {
            "none"
        } else if self.days.is_empty() {
            "empty"
        } else {
            "ok"
        }
    }

    pub fn days(&self) -> &[Day] {
        &self.days
    }

    pub fn day(&self, date: &str) -> Option<&Day> {
        self.days.iter().find(|d| d.date == date)
    }

    /// The boat that sailed a day, out of the front matter of its note —
    /// `<vault>/YYYY/MM/YYYY-MM-DD.md`, as omalogbook writes it. Falls
    /// back to the boat omalogbook is configured for.
    pub fn boat(&self, date: &str) -> String {
        let note = self
            .vault
            .join(&date[..4])
            .join(&date[5..7])
            .join(format!("{date}.md"));
        std::fs::read_to_string(note)
            .ok()
            .and_then(|text| front_matter(&text, "boat"))
            .unwrap_or_else(boat_name)
    }

    /// Re-reads the tracks if any file has appeared, grown or changed. A
    /// running log grows today's file every few seconds, so this is asked
    /// before every answer; unchanged, it costs one directory listing.
    pub fn refresh(&mut self) {
        let mut found: Vec<(PathBuf, u64, Option<SystemTime>)> = Vec::new();
        if let Ok(rd) = std::fs::read_dir(&self.tracks) {
            for e in rd.flatten() {
                let path = e.path();
                if path
                    .extension()
                    .is_none_or(|x| !x.eq_ignore_ascii_case("gpx"))
                {
                    continue;
                }
                let (len, modified) = e
                    .metadata()
                    .map_or((0, None), |m| (m.len(), m.modified().ok()));
                found.push((path, len, modified));
            }
        }
        found.sort();
        let mut h = Fnv::default();
        for (p, len, t) in &found {
            h.write(p.as_os_str().as_encoded_bytes());
            h.write(&len.to_le_bytes());
            h.write(format!("{t:?}").as_bytes());
        }
        let stamp = format!("{} {:016x}", found.len(), h.0);
        // The first stamp is never empty, so the first call always reads.
        if stamp == self.stamp {
            return;
        }
        self.stamp = stamp;

        let mut files = HashMap::with_capacity(found.len());
        let mut passages: Vec<Filed> = Vec::new();
        self.skipped = 0;
        for (path, len, modified) in found {
            let cached = self
                .files
                .get(&path)
                .filter(|e| e.len == len && e.modified == modified)
                .cloned();
            let entry = cached.unwrap_or_else(|| Entry {
                len,
                modified,
                segments: Arc::new(
                    std::fs::read_to_string(&path)
                        .map(|t| parse_gpx(&t))
                        .unwrap_or_default(),
                ),
            });
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            let start = entry
                .segments
                .iter()
                .flatten()
                .map(|p| p.epoch)
                .find(|e| *e > 0)
                .unwrap_or(0);
            // The name says which day it belongs to, as omalogbook wrote
            // it; a file from somewhere else is filed by its first fix.
            let date = date_from_name(&name).or_else(|| (start > 0).then(|| local_date(start)));
            match date.filter(|_| entry.segments.iter().any(|s| !s.is_empty())) {
                Some(date) => passages.push(Filed {
                    date,
                    start,
                    segments: entry.segments.clone(),
                }),
                None => self.skipped += 1,
            }
            files.insert(path, entry);
        }
        self.files = files;
        // Oldest day first, and within a day the passages in the order
        // they were sailed. A passage with no times keeps its name's order.
        passages.sort_by(|a, b| a.date.cmp(&b.date).then(a.start.cmp(&b.start)));
        self.days = build(passages);
    }
}

/// A day's passages, joined: every continuous run in order, with a gap
/// between each pair.
fn build(passages: Vec<Filed>) -> Vec<Day> {
    let mut days: Vec<Day> = Vec::new();
    for filed in passages {
        let date = filed.date;
        let mut runs: Vec<Vec<Point>> = Vec::new();
        for segment in filed.segments.iter() {
            // A hole inside a passage is a gap too: omalogbook keeps the
            // passage open through a short loss of fix, and the line
            // across it is no more real than the one between two files.
            let mut run: Vec<Point> = Vec::new();
            for p in segment {
                if let Some(last) = run.last()
                    && last.epoch > 0
                    && p.epoch > 0
                    && p.epoch - last.epoch >= GAP_SECONDS
                {
                    runs.push(std::mem::take(&mut run));
                }
                run.push(*p);
            }
            if !run.is_empty() {
                runs.push(run);
            }
        }
        if runs.is_empty() {
            continue;
        }
        if days.last().is_none_or(|d| d.date != date) {
            days.push(Day {
                date,
                passages: 0,
                legs: Vec::new(),
            });
        }
        let day = days.last_mut().expect("there is a day by now");
        day.passages += 1;
        for run in runs {
            if let Some(previous) = day.legs.last().and_then(|l| l.points.last()).copied() {
                let next = run[0];
                // Two fixes in the same spot need no line drawn between
                // them, but they are still a hole in the day.
                day.legs.push(Leg {
                    kind: Kind::Gap,
                    points: vec![previous, next],
                });
            }
            day.legs.push(Leg {
                kind: Kind::Run,
                points: run,
            });
        }
    }
    days
}

/// Mercator bounds of a day, for the camera.
pub fn day_rect(day: &Day) -> Option<Rect> {
    let b = day.bounds()?;
    let mut r = Rect::EMPTY;
    r.add(geo::mercator(b[0], b[3]));
    r.add(geo::mercator(b[2], b[1]));
    Some(r)
}

#[cfg(test)]
mod tests {
    use super::*;

    const GPX: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<gpx version="1.1" creator="omalogbook" xmlns="http://www.topografix.com/GPX/1/1">
  <trk>
    <name>Dash 2026-09-21 12:07</name>
    <trkseg>
      <trkpt lat="37.866845" lon="-122.313258">
        <time>2026-09-21T19:07:34Z</time>
        <extensions>
          <speed>0.52</speed>
        </extensions>
      </trkpt>
      <trkpt lat="37.866807" lon="-122.313255">
        <time>2026-09-21T19:07:44Z</time>
      </trkpt>
      <trkpt lat="37.869063" lon="-122.450883">
        <time>2026-09-21T20:50:34Z</time>
      </trkpt>
    </trkseg>
  </trk>
</gpx>"#;

    #[test]
    fn reads_a_track() {
        let segments = parse_gpx(GPX);
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].len(), 3);
        assert_eq!(segments[0][0].lat, 37.866845);
        assert_eq!(segments[0][0].lon, -122.313258);
        assert_eq!(segments[0][1].epoch, 1_790_017_664);
        assert_eq!(iso_utc(segments[0][0].epoch), "2026-09-21T19:07:34Z");
    }

    #[test]
    fn reads_the_gpx_other_writers_produce() {
        // Attributes the other way round, single quotes, an empty element,
        // a fraction, a zone, and a second segment.
        let text = "<trkseg><trkpt lon='1.5' lat='2.5'><time>2026-01-02T03:04:05.250+02:00</time></trkpt>\
                    <trkpt lat=\"2.6\" lon=\"1.6\"/></trkseg><trkseg><trkpt lat=\"3\" lon=\"4\"></trkpt></trkseg>";
        let segments = parse_gpx(text);
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0].len(), 2);
        assert_eq!(segments[0][0].lat, 2.5);
        assert_eq!(iso_utc(segments[0][0].epoch), "2026-01-02T01:04:05Z");
        // No time is 0, never a made-up one.
        assert_eq!(segments[0][1].epoch, 0);
        assert_eq!(segments[1][0].lat, 3.0);
    }

    #[test]
    fn a_point_out_of_range_is_dropped() {
        let text = "<trkseg><trkpt lat=\"91\" lon=\"0\"></trkpt><trkpt lat=\"1\" lon=\"2\"></trkpt></trkseg>";
        let segments = parse_gpx(text);
        assert_eq!(segments[0].len(), 1);
        assert_eq!(segments[0][0].lat, 1.0);
    }

    #[test]
    fn times_round_trip() {
        for s in [
            "1970-01-01T00:00:00Z",
            "2026-09-21T18:43:30Z",
            "1999-12-31T23:59:59Z",
            "2000-02-29T12:00:00Z",
        ] {
            assert_eq!(iso_utc(epoch_from_iso(s).unwrap()), s, "{s}");
        }
        assert!(epoch_from_iso("2026-09-21").is_none());
        assert!(epoch_from_iso("not a time at all").is_none());
        assert!(epoch_from_iso("2026-13-01T00:00:00Z").is_none());
    }

    fn day_of(files: &[(&str, &str)]) -> Day {
        let passages = files
            .iter()
            .map(|(name, text)| {
                let segments = Arc::new(parse_gpx(text));
                let start = segments
                    .iter()
                    .flatten()
                    .map(|p| p.epoch)
                    .find(|e| *e > 0)
                    .unwrap_or(0);
                Filed {
                    date: date_from_name(name).unwrap(),
                    start,
                    segments,
                }
            })
            .collect();
        let mut days = build(passages);
        assert_eq!(days.len(), 1);
        days.remove(0)
    }

    fn track(name: &str, points: &[(&str, f64, f64)]) -> String {
        let mut out = format!("<trk><name>{name}</name><trkseg>");
        for (t, lat, lon) in points {
            out.push_str(&format!(
                "<trkpt lat=\"{lat}\" lon=\"{lon}\"><time>{t}</time></trkpt>"
            ));
        }
        out.push_str("</trkseg></trk>");
        out
    }

    #[test]
    fn a_day_is_one_trip_with_its_holes_named() {
        let first = track(
            "one",
            &[
                ("2026-09-21T19:00:00Z", 37.80, -122.40),
                ("2026-09-21T19:00:10Z", 37.81, -122.40),
            ],
        );
        let second = track(
            "two",
            &[
                ("2026-09-21T19:30:00Z", 37.83, -122.40),
                ("2026-09-21T19:30:10Z", 37.84, -122.40),
            ],
        );
        let day = day_of(&[
            ("2026-09-21-190000.gpx", first.as_str()),
            ("2026-09-21-193000.gpx", second.as_str()),
        ]);
        assert_eq!(day.passages, 2);
        // Run, gap, run: the straight line is its own leg.
        assert_eq!(
            day.legs.iter().map(|l| l.kind).collect::<Vec<_>>(),
            vec![Kind::Run, Kind::Gap, Kind::Run]
        );
        assert_eq!(day.points(), 4);
        // A minute of latitude is a nautical mile.
        assert!((day.run_nm() - 1.2).abs() < 0.02, "{}", day.run_nm());
        assert!((day.gap_nm() - 1.2).abs() < 0.02, "{}", day.gap_nm());
        let s = day.summary();
        assert_eq!(s["gaps"], 1);
        assert_eq!(s["from"], "2026-09-21T19:00:00Z");
        assert_eq!(s["to"], "2026-09-21T19:30:10Z");
        assert_eq!(s["seconds"], 1810);
        assert!((s["distanceNm"].as_f64().unwrap() - 2.4).abs() < 0.03);
    }

    #[test]
    fn a_hole_inside_a_passage_is_a_gap_too() {
        // omalogbook keeps a passage open through a short loss of fix.
        let text = track(
            "one",
            &[
                ("2026-09-21T19:00:00Z", 37.80, -122.40),
                ("2026-09-21T19:00:10Z", 37.81, -122.40),
                ("2026-09-21T19:20:00Z", 37.90, -122.40),
            ],
        );
        let day = day_of(&[("2026-09-21-190000.gpx", text.as_str())]);
        assert_eq!(day.passages, 1);
        assert_eq!(day.gaps().count(), 1);
        assert_eq!(day.runs().count(), 2);
        // A point missed by a few seconds is not a hole.
        let close = track(
            "one",
            &[
                ("2026-09-21T19:00:00Z", 37.80, -122.40),
                ("2026-09-21T19:00:40Z", 37.81, -122.40),
            ],
        );
        assert_eq!(
            day_of(&[("2026-09-21-190000.gpx", close.as_str())])
                .gaps()
                .count(),
            0
        );
    }

    #[test]
    fn the_drawing_keeps_the_shape_and_the_ends() {
        let mut points = Vec::new();
        for i in 0..400 {
            points.push((
                format!("2026-09-21T19:{:02}:{:02}Z", i / 60, i % 60),
                37.80 + f64::from(i) * 0.0001,
                -122.40,
            ));
        }
        let borrowed: Vec<(&str, f64, f64)> = points
            .iter()
            .map(|(t, a, o)| (t.as_str(), *a, *o))
            .collect();
        let day = day_of(&[("2026-09-21-190000.gpx", track("one", &borrowed).as_str())]);
        let drawn = day.drawing(12);
        let runs = drawn["runs"].as_array().unwrap();
        assert_eq!(runs.len(), 1);
        let line = runs[0].as_array().unwrap();
        // A straight line keeps its two ends and little else.
        assert!(line.len() <= 8, "{}", line.len());
        assert_eq!(line[0].as_f64().unwrap(), 37.8);
        assert_eq!(line[line.len() - 2].as_f64().unwrap(), 37.8399);
        // Zoomed in, more of it is worth drawing than at a distance.
        let close = day.drawing(18);
        assert!(close["drawn"].as_u64() >= drawn["drawn"].as_u64());
    }

    #[test]
    fn a_hole_the_boat_sat_through_draws_nothing() {
        // The log closed and opened again at the slip, in the same spot.
        let first = track("one", &[("2026-09-21T19:00:00Z", 37.80, -122.40)]);
        let second = track("two", &[("2026-09-21T19:30:00Z", 37.80, -122.40)]);
        let day = day_of(&[
            ("2026-09-21-190000.gpx", first.as_str()),
            ("2026-09-21-193000.gpx", second.as_str()),
        ]);
        assert_eq!(day.gaps().count(), 1);
        assert_eq!(day.gap_nm(), 0.0);
        let drawn = day.drawing(15);
        assert!(drawn["gaps"].as_array().unwrap().is_empty());
    }

    #[test]
    fn the_export_is_one_segment_and_says_what_was_filled() {
        let first = track("one", &[("2026-09-21T19:00:00Z", 37.80, -122.40)]);
        let second = track("two", &[("2026-09-21T19:30:00Z", 37.83, -122.40)]);
        let day = day_of(&[
            ("2026-09-21-190000.gpx", first.as_str()),
            ("2026-09-21-193000.gpx", second.as_str()),
        ]);
        let gpx = day.gpx("Dash");
        assert_eq!(gpx.matches("<trkseg>").count(), 1);
        // Each fix once: the gap's ends are the runs' own points.
        assert_eq!(gpx.matches("<trkpt").count(), 2);
        assert!(gpx.contains("1 gap filled"), "{gpx}");
        assert!(gpx.contains("<name>Dash 2026-09-21</name>"));
    }

    #[test]
    fn the_note_says_whose_boat_it_was() {
        let note = "---\ndate: 2026-09-21\nboat: Dash\ndistance_nm: 18.3\n---\n\n# the day\nboat: Someone else's\n";
        assert_eq!(front_matter(note, "boat").as_deref(), Some("Dash"));
        assert_eq!(front_matter(note, "date").as_deref(), Some("2026-09-21"));
        assert_eq!(front_matter(note, "missing"), None);
        // Nothing to read out of a note with no front matter.
        assert_eq!(front_matter("boat: Dash\n", "boat"), None);
    }

    #[test]
    fn a_name_says_which_day() {
        assert_eq!(
            date_from_name("2026-09-21-114330.gpx").as_deref(),
            Some("2026-09-21")
        );
        assert_eq!(date_from_name("saturday.gpx"), None);
        assert_eq!(date_from_name("2026-13-99-000000.gpx"), None);
        assert!(is_date("2026-09-21"));
        assert!(!is_date("2026-9-21"));
        assert!(!is_date("../../etc"));
    }
}
