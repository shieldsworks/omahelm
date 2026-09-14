//! S-57, the IHO transfer standard NOAA's electronic navigational charts
//! (ENCs) are written in. A cell is a base file (`.000`) and its updates
//! (`.001`, `.002`, ...). Reading one applies the updates and assembles
//! each feature's geometry from the shared nodes and edges.

pub mod codes;
pub mod names;

use crate::iso8211::{Field, FieldDefn, Format, Module, Value};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

pub type Result<T> = std::result::Result<T, String>;

/// The long name of a feature: producing agency, number and subdivision.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FeatureId {
    pub agen: u16,
    pub fidn: u32,
    pub fids: u16,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LonLat {
    pub lon: f64,
    pub lat: f64,
}

#[derive(Clone, Debug)]
pub enum Geometry {
    None,
    Point(LonLat),
    /// Depths in metres, positive down.
    Soundings(Vec<(LonLat, f64)>),
    Lines(Vec<Vec<LonLat>>),
    /// `rings` are closed and filled even-odd. `outline` is the boundary to
    /// stroke: the rings without masked edges or the cell's data limit.
    Area {
        rings: Vec<Vec<LonLat>>,
        outline: Vec<Vec<LonLat>>,
    },
}

#[derive(Clone, Debug)]
pub struct Feature {
    /// The object class code (OBJL), for example 42 for DEPARE.
    pub class: u16,
    pub id: FeatureId,
    /// Attribute code and value, national attributes included. An empty
    /// value means the value is unknown.
    pub attrs: Vec<(u16, String)>,
    pub geometry: Geometry,
    /// Features this one is the master of: the lights and topmarks on a
    /// buoy, for example.
    pub slaves: Vec<FeatureId>,
}

impl Feature {
    pub fn attr(&self, code: u16) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(c, v)| *c == code && !v.is_empty())
            .map(|(_, v)| v.as_str())
    }
}

/// A cell with its updates applied.
#[derive(Debug)]
pub struct Cell {
    /// The dataset name, such as `US5OAKFI`.
    pub name: String,
    pub edition: u32,
    /// The last update applied.
    pub update: u32,
    /// The issue date of the last file applied, `YYYYMMDD`.
    pub issued: String,
    /// The compilation scale, such as 12000 for 1:12,000.
    pub scale: u32,
    pub features: Vec<Feature>,
}

impl Cell {
    /// Reads a base cell and every update file beside it.
    pub fn open(base: &Path) -> Result<Cell> {
        let mut reader = Reader::default();
        let bytes = std::fs::read(base).map_err(|e| format!("{}: {e}", base.display()))?;
        reader
            .read(&bytes, true)
            .map_err(|e| format!("{}: {e}", base.display()))?;
        for path in updates(base) {
            let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            reader
                .read(&bytes, false)
                .map_err(|e| format!("{}: {e}", path.display()))?;
            if reader.cancelled {
                return Err(format!("{} was cancelled by its producer", reader.name));
            }
        }
        Ok(reader.build())
    }
}

/// The update files beside a base cell, in order. A reissued base cell
/// already holds its early updates, so the first may be `.032`, not `.001`;
/// the reader skips the ones it has and refuses a gap.
pub fn updates(base: &Path) -> Vec<PathBuf> {
    (1..1000)
        .map(|n| base.with_extension(format!("{n:03}")))
        .filter(|p| p.is_file())
        .collect()
}

#[derive(Clone, Copy, Debug)]
struct Pointer {
    rcnm: u8,
    rcid: u32,
    ornt: u8,
    usag: u8,
    topi: u8,
    mask: u8,
}

#[derive(Default)]
struct RawFeature {
    prim: u8,
    objl: u16,
    id: FeatureId,
    attrs: Vec<(u16, String)>,
    spatial: Vec<Pointer>,
    related: Vec<(FeatureId, u8)>,
}

#[derive(Default)]
struct RawVector {
    attrs: Vec<(u16, String)>,
    pointers: Vec<Pointer>,
    /// (y, x) in units of 1/COMF degrees.
    coords: Vec<(i32, i32)>,
    /// Soundings only: depth in units of 1/SOMF metres, one per coordinate.
    depths: Vec<i32>,
}

/// An update's control: insert, delete or modify `count` entries at the
/// 1-based index.
#[derive(Clone, Copy)]
struct Control {
    op: u8,
    index: usize,
    count: usize,
}

struct Reader {
    name: String,
    edition: u32,
    update: u32,
    issued: String,
    scale: u32,
    comf: f64,
    somf: f64,
    /// Lexical levels: 0 ASCII, 1 ISO 8859-1, 2 UCS-2.
    aall: u8,
    nall: u8,
    features: BTreeMap<u32, RawFeature>,
    vectors: HashMap<(u8, u32), RawVector>,
    cancelled: bool,
    skip: bool,
}

impl Default for Reader {
    fn default() -> Self {
        Reader {
            name: String::new(),
            edition: 0,
            update: 0,
            issued: String::new(),
            scale: 0,
            comf: 10_000_000.0,
            somf: 10.0,
            aall: 1,
            nall: 2,
            features: BTreeMap::new(),
            vectors: HashMap::new(),
            cancelled: false,
            skip: false,
        }
    }
}

const DELETE: &str = "\u{7f}";

/// Named subfields of one row.
struct Row<'m, 'a> {
    defn: &'m FieldDefn,
    values: Vec<Value<'a>>,
}

impl<'a> Row<'_, 'a> {
    fn get(&self, name: &str) -> Option<Value<'a>> {
        self.defn
            .index(name)
            .and_then(|i| self.values.get(i).copied())
    }

    fn int(&self, name: &str) -> i64 {
        self.get(name).map(|v| v.int()).unwrap_or(0)
    }

    fn text(&self, name: &str) -> String {
        self.get(name).map(|v| v.text()).unwrap_or_default()
    }
}

fn rows<'m, 'a>(module: &'m Module, field: &Field<'a>) -> Result<Vec<Row<'m, 'a>>> {
    let defn = module.defn(field.tag)?;
    Ok(defn
        .rows(field.data)?
        .into_iter()
        .map(|values| Row { defn, values })
        .collect())
}

fn first<'m, 'a>(module: &'m Module, field: &Field<'a>) -> Result<Row<'m, 'a>> {
    rows(module, field)?
        .into_iter()
        .next()
        .ok_or_else(|| format!("empty {} field", field.tag))
}

/// A NAME subfield: record type, then record id.
fn name(bits: &[u8]) -> (u8, u32) {
    if bits.len() < 5 {
        return (0, 0);
    }
    (
        bits[0],
        u32::from_le_bytes([bits[1], bits[2], bits[3], bits[4]]),
    )
}

/// An LNAM subfield: a feature's long name.
fn long_name(bits: &[u8]) -> FeatureId {
    if bits.len() < 8 {
        return FeatureId::default();
    }
    FeatureId {
        agen: u16::from_le_bytes([bits[0], bits[1]]),
        fidn: u32::from_le_bytes([bits[2], bits[3], bits[4], bits[5]]),
        fids: u16::from_le_bytes([bits[6], bits[7]]),
    }
}

/// ATTF, NATF and ATTV fields: an attribute code, then its value in the
/// given lexical level, repeated.
fn attributes(data: &[u8], level: u8) -> Vec<(u16, String)> {
    let mut out = Vec::new();
    let mut i = 0;
    // An attribute is its code and a terminated value, so a shorter tail is
    // what's left of a field terminator. (A code can't be tested for the
    // terminator byte: attribute 30 starts with it.)
    let smallest = if level == 2 { 4 } else { 3 };
    while i + smallest <= data.len() {
        let code = u16::from_le_bytes([data[i], data[i + 1]]);
        i += 2;
        let text = if level == 2 {
            let mut units = Vec::new();
            while i + 1 < data.len() {
                let unit = u16::from_le_bytes([data[i], data[i + 1]]);
                i += 2;
                if unit == 0x1f || unit == 0x1e {
                    break;
                }
                units.push(unit);
            }
            String::from_utf16_lossy(&units)
        } else {
            let end = data[i..]
                .iter()
                .position(|&b| b == 0x1f || b == 0x1e)
                .map_or(data.len(), |p| i + p);
            let text = data[i..end].iter().map(|&b| b as char).collect();
            i = end + 1;
            text
        };
        out.push((code, text.trim_end_matches('\0').to_string()));
    }
    out
}

fn control(row: &Row, op: &str, index: &str, count: &str) -> Control {
    Control {
        op: row.int(op) as u8,
        index: row.int(index).max(1) as usize,
        count: row.int(count).max(0) as usize,
    }
}

/// Applies an update control to a list of pointers or coordinates.
fn apply<T: Clone>(list: &mut Vec<T>, control: Control, new: &[T]) {
    let at = (control.index - 1).min(list.len());
    match control.op {
        1 => {
            list.splice(at..at, new.iter().cloned());
        }
        2 => {
            let end = (at + control.count).min(list.len());
            list.drain(at..end);
        }
        3 => {
            for (i, item) in new.iter().enumerate().take(control.count) {
                if let Some(slot) = list.get_mut(at + i) {
                    *slot = item.clone();
                }
            }
        }
        _ => {}
    }
}

/// Sets, replaces or (with the delete marker) removes attributes.
fn merge(attrs: &mut Vec<(u16, String)>, changes: Vec<(u16, String)>) {
    for (code, value) in changes {
        attrs.retain(|(c, _)| *c != code);
        if value != DELETE {
            attrs.push((code, value));
        }
    }
}

fn coordinates(field: &Field, dims: usize) -> Result<Vec<(i32, i32, i32)>> {
    let step = 4 * dims;
    if !field.data.len().is_multiple_of(step) {
        return Err(format!("{} field is not whole coordinates", field.tag));
    }
    let int = |b: &[u8]| i32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    Ok(field
        .data
        .chunks_exact(step)
        .map(|c| {
            let z = if dims == 3 { int(&c[8..12]) } else { 0 };
            (int(&c[0..4]), int(&c[4..8]), z)
        })
        .collect())
}

impl Reader {
    fn read(&mut self, bytes: &[u8], base: bool) -> Result<()> {
        let mut module = Module::open(bytes)?;
        for (tag, dims) in [("SG2D", 2), ("SG3D", 3)] {
            if let Ok(defn) = module.defn(tag)
                && (defn.formats.len() != dims
                    || defn.formats.iter().any(|f| *f != Format::Signed(4)))
            {
                return Err(format!("unsupported {tag} encoding"));
            }
        }
        self.skip = false;
        while let Some(fields) = module.next_record()? {
            let Some(head) = fields.iter().find(|f| f.tag != "0001") else {
                continue;
            };
            match head.tag {
                "DSID" => self.dataset(&module, &fields, base)?,
                _ if self.skip => return Ok(()),
                "DSPM" => self.parameters(&module, &fields)?,
                "VRID" => self.vector(&module, &fields)?,
                "FRID" => self.feature(&module, &fields)?,
                _ => {}
            }
        }
        Ok(())
    }

    fn dataset(&mut self, module: &Module, fields: &[Field], base: bool) -> Result<()> {
        for field in fields {
            match field.tag {
                "DSID" => {
                    let row = first(module, field)?;
                    let edition = row.text("EDTN").trim().parse().unwrap_or(0);
                    let update = row.text("UPDN").trim().parse().unwrap_or(0);
                    if base {
                        self.name = row.text("DSNM");
                        if let Some(stem) = self.name.strip_suffix(".000") {
                            self.name = stem.to_string();
                        }
                        self.edition = edition;
                        self.update = update;
                    } else {
                        let name = row.text("DSNM");
                        let stem = name.split('.').next().unwrap_or("");
                        if stem != self.name {
                            return Err(format!("update for {stem}, not {}", self.name));
                        }
                        if edition == 0 {
                            self.cancelled = true;
                        } else if edition != self.edition {
                            return Err(format!(
                                "update {update} is for edition {edition}, but the cell is edition {}; download it again",
                                self.edition
                            ));
                        } else if update <= self.update {
                            // Already in a reissued base cell.
                            self.skip = true;
                            return Ok(());
                        } else if update != self.update + 1 {
                            return Err(format!(
                                "update {} is missing before update {update}; download the cell again",
                                self.update + 1
                            ));
                        }
                        self.update = update;
                    }
                    self.issued = row.text("ISDT");
                }
                "DSSI" => {
                    let row = first(module, field)?;
                    self.aall = row.int("AALL") as u8;
                    self.nall = row.int("NALL") as u8;
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn parameters(&mut self, module: &Module, fields: &[Field]) -> Result<()> {
        if let Some(field) = fields.iter().find(|f| f.tag == "DSPM") {
            let row = first(module, field)?;
            let comf = row.int("COMF");
            let somf = row.int("SOMF");
            if comf > 0 {
                self.comf = comf as f64;
            }
            if somf > 0 {
                self.somf = somf as f64;
            }
            self.scale = row.int("CSCL") as u32;
        }
        Ok(())
    }

    fn vector(&mut self, module: &Module, fields: &[Field]) -> Result<()> {
        let mut key = (0u8, 0u32);
        let mut ruin = 1;
        let mut attrs = Vec::new();
        let mut pointers = Vec::new();
        let mut coords = Vec::new();
        let mut depths = Vec::new();
        let (mut vrpc, mut sgcc) = (None, None);
        let mut has_coords = false;
        for field in fields {
            match field.tag {
                "VRID" => {
                    let row = first(module, field)?;
                    key = (row.int("RCNM") as u8, row.int("RCID") as u32);
                    ruin = row.int("RUIN");
                }
                "ATTV" => attrs.extend(attributes(field.data, self.aall)),
                "VRPC" => vrpc = Some(control(&first(module, field)?, "VPUI", "VPIX", "NVPT")),
                "VRPT" => {
                    for row in rows(module, field)? {
                        let (rcnm, rcid) = name(row.get("NAME").map(|v| v.bits()).unwrap_or(&[]));
                        pointers.push(Pointer {
                            rcnm,
                            rcid,
                            ornt: row.int("ORNT") as u8,
                            usag: row.int("USAG") as u8,
                            topi: row.int("TOPI") as u8,
                            mask: row.int("MASK") as u8,
                        });
                    }
                }
                "SGCC" => sgcc = Some(control(&first(module, field)?, "CCUI", "CCIX", "CCNC")),
                "SG2D" => {
                    has_coords = true;
                    coords.extend(coordinates(field, 2)?.into_iter().map(|(y, x, _)| (y, x)));
                }
                "SG3D" => {
                    has_coords = true;
                    for (y, x, z) in coordinates(field, 3)? {
                        coords.push((y, x));
                        depths.push(z);
                    }
                }
                _ => {}
            }
        }
        match ruin {
            1 => {
                self.vectors.insert(
                    key,
                    RawVector {
                        attrs,
                        pointers,
                        coords,
                        depths,
                    },
                );
            }
            2 => {
                self.vectors.remove(&key);
            }
            3 => {
                let Some(v) = self.vectors.get_mut(&key) else {
                    return Ok(());
                };
                merge(&mut v.attrs, attrs);
                if let Some(c) = vrpc {
                    apply(&mut v.pointers, c, &pointers);
                }
                match sgcc {
                    Some(c) => {
                        apply(&mut v.coords, c, &coords);
                        if !v.depths.is_empty() || !depths.is_empty() {
                            apply(&mut v.depths, c, &depths);
                        }
                    }
                    None if has_coords => {
                        v.coords = coords;
                        v.depths = depths;
                    }
                    None => {}
                }
            }
            other => return Err(format!("unknown update instruction {other}")),
        }
        Ok(())
    }

    fn feature(&mut self, module: &Module, fields: &[Field]) -> Result<()> {
        let mut rcid = 0u32;
        let mut ruin = 1;
        let mut raw = RawFeature::default();
        let mut attrs = Vec::new();
        let (mut fspc, mut ffpc) = (None, None);
        let mut spatial = Vec::new();
        let mut related = Vec::new();
        for field in fields {
            match field.tag {
                "FRID" => {
                    let row = first(module, field)?;
                    rcid = row.int("RCID") as u32;
                    ruin = row.int("RUIN");
                    raw.prim = row.int("PRIM") as u8;
                    raw.objl = row.int("OBJL") as u16;
                }
                "FOID" => {
                    let row = first(module, field)?;
                    raw.id = FeatureId {
                        agen: row.int("AGEN") as u16,
                        fidn: row.int("FIDN") as u32,
                        fids: row.int("FIDS") as u16,
                    };
                }
                "ATTF" => attrs.extend(attributes(field.data, self.aall)),
                "NATF" => attrs.extend(attributes(field.data, self.nall)),
                "FSPC" => fspc = Some(control(&first(module, field)?, "FSUI", "FSIX", "NSPT")),
                "FSPT" => {
                    for row in rows(module, field)? {
                        let (rcnm, id) = name(row.get("NAME").map(|v| v.bits()).unwrap_or(&[]));
                        spatial.push(Pointer {
                            rcnm,
                            rcid: id,
                            ornt: row.int("ORNT") as u8,
                            usag: row.int("USAG") as u8,
                            topi: 0,
                            mask: row.int("MASK") as u8,
                        });
                    }
                }
                "FFPC" => ffpc = Some(control(&first(module, field)?, "FFUI", "FFIX", "NFPT")),
                "FFPT" => {
                    for row in rows(module, field)? {
                        let lnam = long_name(row.get("LNAM").map(|v| v.bits()).unwrap_or(&[]));
                        related.push((lnam, row.int("RIND") as u8));
                    }
                }
                _ => {}
            }
        }
        match ruin {
            1 => {
                raw.attrs = attrs;
                raw.spatial = spatial;
                raw.related = related;
                self.features.insert(rcid, raw);
            }
            2 => {
                self.features.remove(&rcid);
            }
            3 => {
                let Some(f) = self.features.get_mut(&rcid) else {
                    return Ok(());
                };
                merge(&mut f.attrs, attrs);
                if let Some(c) = fspc {
                    apply(&mut f.spatial, c, &spatial);
                }
                if let Some(c) = ffpc {
                    apply(&mut f.related, c, &related);
                }
            }
            other => return Err(format!("unknown update instruction {other}")),
        }
        Ok(())
    }

    fn build(self) -> Cell {
        let comf = self.comf;
        let somf = self.somf;
        let lonlat = |(y, x): (i32, i32)| LonLat {
            lon: x as f64 / comf,
            lat: y as f64 / comf,
        };
        let node = |rcnm: u8, rcid: u32| {
            self.vectors
                .get(&(rcnm, rcid))
                .and_then(|v| v.coords.first().copied())
        };
        // Every edge in full: its first node, its own points, its last node.
        let mut edges: HashMap<u32, Vec<(i32, i32)>> = HashMap::new();
        for (&(rcnm, rcid), v) in &self.vectors {
            if rcnm != 130 {
                continue;
            }
            let (mut begin, mut end) = (None, None);
            for (i, p) in v.pointers.iter().enumerate() {
                let at = node(p.rcnm, p.rcid);
                match (p.topi, i) {
                    (1, _) | (255 | 0, 0) => begin = at,
                    (2, _) | (255 | 0, 1) => end = at,
                    _ => {}
                }
            }
            let mut pts = Vec::with_capacity(v.coords.len() + 2);
            pts.extend(begin);
            pts.extend_from_slice(&v.coords);
            pts.extend(end);
            edges.insert(rcid, pts);
        }
        let oriented = |p: &Pointer| -> Option<Vec<(i32, i32)>> {
            let mut pts = edges.get(&p.rcid)?.clone();
            if p.ornt == 2 {
                pts.reverse();
            }
            Some(pts)
        };

        let mut features = Vec::with_capacity(self.features.len());
        for raw in self.features.values() {
            let geometry = match raw.prim {
                1 => match raw
                    .spatial
                    .first()
                    .and_then(|p| self.vectors.get(&(p.rcnm, p.rcid)))
                {
                    Some(v) if !v.depths.is_empty() => Geometry::Soundings(
                        v.coords
                            .iter()
                            .zip(&v.depths)
                            .map(|(&c, &d)| (lonlat(c), d as f64 / somf))
                            .collect(),
                    ),
                    Some(v) => v
                        .coords
                        .first()
                        .map_or(Geometry::None, |&c| Geometry::Point(lonlat(c))),
                    None => Geometry::None,
                },
                2 => {
                    let parts: Vec<_> = raw.spatial.iter().filter_map(oriented).collect();
                    Geometry::Lines(
                        chain(parts)
                            .into_iter()
                            .map(|l| l.into_iter().map(lonlat).collect())
                            .collect(),
                    )
                }
                3 => {
                    let parts: Vec<_> = raw.spatial.iter().filter_map(oriented).collect();
                    let outline = chain(
                        raw.spatial
                            .iter()
                            .filter(|p| p.mask != 1 && p.usag != 3)
                            .filter_map(oriented)
                            .collect(),
                    );
                    let convert = |ls: Vec<Vec<(i32, i32)>>| {
                        ls.into_iter()
                            .map(|l| l.into_iter().map(lonlat).collect())
                            .collect()
                    };
                    Geometry::Area {
                        rings: convert(rings(parts)),
                        outline: convert(outline),
                    }
                }
                _ => Geometry::None,
            };
            features.push(Feature {
                class: raw.objl,
                id: raw.id,
                attrs: raw.attrs.clone(),
                geometry,
                slaves: raw
                    .related
                    .iter()
                    .filter(|(_, rind)| *rind == 2)
                    .map(|(id, _)| *id)
                    .collect(),
            });
        }
        Cell {
            name: self.name,
            edition: self.edition,
            update: self.update,
            issued: self.issued,
            scale: self.scale,
            features,
        }
    }
}

type Pt = (i32, i32);

/// Joins edges that meet end to start into polylines, in order.
fn chain(parts: Vec<Vec<Pt>>) -> Vec<Vec<Pt>> {
    let mut out: Vec<Vec<Pt>> = Vec::new();
    for part in parts {
        if part.is_empty() {
            continue;
        }
        match out.last_mut() {
            Some(line) if line.last() == part.first() => line.extend_from_slice(&part[1..]),
            _ => out.push(part),
        }
    }
    out
}

/// Links an area's edges into closed rings. S-57 lists them in order, so
/// the next edge almost always continues the ring; when it doesn't, the
/// remaining edges are searched by their end points.
fn rings(parts: Vec<Vec<Pt>>) -> Vec<Vec<Pt>> {
    let mut pending: Vec<Option<Vec<Pt>>> = parts
        .into_iter()
        .filter(|p| p.len() >= 2)
        .map(Some)
        .collect();
    let mut out = Vec::new();
    let mut current: Vec<Pt> = Vec::new();
    let mut next = 0;
    loop {
        if current.is_empty() {
            match pending.iter_mut().find_map(|p| p.take()) {
                Some(part) => current = part,
                None => break,
            }
        }
        if current.len() > 2 && current.first() == current.last() {
            out.push(std::mem::take(&mut current));
            continue;
        }
        let end = *current.last().expect("current ring is not empty");
        let found = (next..pending.len())
            .chain(0..next)
            .find_map(|i| match &pending[i] {
                Some(p) if p.first() == Some(&end) => Some((i, false)),
                Some(p) if p.last() == Some(&end) => Some((i, true)),
                _ => None,
            });
        match found {
            Some((i, reversed)) => {
                let mut part = pending[i].take().expect("found a pending edge");
                if reversed {
                    part.reverse();
                }
                current.extend_from_slice(&part[1..]);
                next = i + 1;
            }
            None => {
                // An open ring still fills: the fill closes it.
                let start = current[0];
                current.push(start);
                out.push(std::mem::take(&mut current));
            }
        }
    }
    out
}

/// An object class's acronym, such as `DEPARE`.
pub fn acronym(class: u16) -> &'static str {
    codes::OBJECT_CLASSES
        .iter()
        .find(|(c, _, _)| *c == class)
        .map_or("?", |(_, a, _)| a)
}

pub fn class_name(class: u16) -> &'static str {
    codes::OBJECT_CLASSES
        .iter()
        .find(|(c, _, _)| *c == class)
        .map_or("Unknown object", |(_, _, n)| n)
}

pub fn class_code(acronym: &str) -> Option<u16> {
    codes::OBJECT_CLASSES
        .iter()
        .find(|(_, a, _)| *a == acronym)
        .map(|(c, _, _)| *c)
}

pub fn attribute(code: u16) -> Option<(&'static str, &'static str, u8)> {
    codes::ATTRIBUTES
        .iter()
        .find(|(c, _, _, _)| *c == code)
        .map(|(_, a, n, t)| (*a, *n, *t))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rings_close_out_of_order_and_reversed_edges() {
        // A square from three edges, the last listed backwards.
        let a = vec![(0, 0), (0, 10)];
        let b = vec![(10, 0), (10, 10), (0, 10)];
        let c = vec![(10, 0), (0, 0)];
        let r = rings(vec![a, c, b]);
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].first(), r[0].last());
        assert_eq!(r[0].len(), 5);
    }

    #[test]
    fn chain_joins_touching_edges_only() {
        let lines = chain(vec![
            vec![(0, 0), (1, 1)],
            vec![(1, 1), (2, 2)],
            vec![(5, 5), (6, 6)],
        ]);
        assert_eq!(
            lines,
            vec![vec![(0, 0), (1, 1), (2, 2)], vec![(5, 5), (6, 6)]]
        );
    }

    #[test]
    fn update_controls() {
        let mut list = vec![1, 2, 3, 4];
        apply(
            &mut list,
            Control {
                op: 1,
                index: 2,
                count: 2,
            },
            &[8, 9],
        );
        assert_eq!(list, vec![1, 8, 9, 2, 3, 4]);
        apply(
            &mut list,
            Control {
                op: 2,
                index: 1,
                count: 3,
            },
            &[],
        );
        assert_eq!(list, vec![2, 3, 4]);
        apply(
            &mut list,
            Control {
                op: 3,
                index: 3,
                count: 1,
            },
            &[7],
        );
        assert_eq!(list, vec![2, 3, 7]);
    }

    #[test]
    fn attribute_levels_and_delete_marker() {
        let latin = attributes(b"\x74\x00Caf\xe9\x1f", 1);
        assert_eq!(latin, vec![(116, "Café".to_string())]);
        let ucs2 = attributes(b"\x2d\x01B\x00a\x00y\x00\x1f\x00\x1e\x00", 2);
        assert_eq!(ucs2, vec![(301, "Bay".to_string())]);
        // Attribute 30 (CATHAF) starts with the field terminator's byte.
        let harbour = attributes(b"\x1e\x005\x1f\x74\x00Emeryville Marina\x1f", 1);
        assert_eq!(
            harbour,
            vec![
                (30, "5".to_string()),
                (116, "Emeryville Marina".to_string())
            ]
        );
        let mut attrs = vec![(116, "Old".to_string()), (75, "3".to_string())];
        merge(
            &mut attrs,
            vec![(116, DELETE.to_string()), (75, "4".to_string())],
        );
        assert_eq!(attrs, vec![(75, "4".to_string())]);
    }
}
