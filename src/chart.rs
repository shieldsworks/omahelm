//! A cell ready to draw: its features projected to Web Mercator, each with
//! its bounds, and the cell's coverage.

use crate::geo::{Rect, mercator};
use crate::s57::names::*;
use crate::s57::{Cell, FeatureId, Geometry, LonLat};
use std::collections::HashMap;

pub type P = [f64; 2];

#[derive(Debug)]
pub enum Geom {
    None,
    Point(P),
    /// Depths in metres.
    Soundings(Vec<(P, f32)>),
    Lines(Vec<Vec<P>>),
    Area {
        rings: Vec<Vec<P>>,
        outline: Vec<Vec<P>>,
    },
}

#[derive(Debug)]
pub struct Item {
    pub class: u16,
    pub id: FeatureId,
    pub attrs: Vec<(u16, String)>,
    pub geom: Geom,
    pub bbox: Rect,
    /// Hidden when the display scale is smaller than 1:scamin.
    pub scamin: f64,
    /// Indices of this item's slaves in the same chart: a buoy's light.
    pub slaves: Vec<usize>,
    /// Index of the master this item hangs off, if any.
    pub master: Option<usize>,
}

impl Item {
    pub fn attr(&self, code: u16) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(c, v)| *c == code && !v.is_empty())
            .map(|(_, v)| v.as_str())
    }

    pub fn num(&self, code: u16) -> Option<f64> {
        self.attr(code)?.trim().parse().ok()
    }

    /// An enumerated or list attribute as numbers: `3,4` is [3, 4].
    pub fn list(&self, code: u16) -> Vec<u32> {
        self.attr(code)
            .map(|v| v.split(',').filter_map(|n| n.trim().parse().ok()).collect())
            .unwrap_or_default()
    }

    pub fn first(&self, code: u16) -> Option<u32> {
        self.list(code).first().copied()
    }

    /// The point a symbol or label sits at.
    pub fn anchor(&self) -> Option<P> {
        match &self.geom {
            Geom::Point(p) => Some(*p),
            Geom::Area { .. } | Geom::Lines(_) => Some(self.bbox.center()),
            _ => None,
        }
    }
}

#[derive(Debug)]
pub struct Chart {
    pub name: String,
    pub scale: u32,
    pub edition: u32,
    pub update: u32,
    pub issued: String,
    pub bounds: Rect,
    /// Where the cell has data (M_COVR, CATCOV 1).
    pub coverage: Vec<Vec<P>>,
    pub items: Vec<Item>,
    /// The depth contours the cell carries, in metres, shallow first.
    pub contours: Vec<f64>,
}

fn project(p: &LonLat) -> P {
    mercator(p.lon, p.lat)
}

fn projected(lines: &[Vec<LonLat>]) -> Vec<Vec<P>> {
    lines
        .iter()
        .map(|l| l.iter().map(project).collect())
        .collect()
}

impl Chart {
    pub fn from_cell(cell: Cell) -> Chart {
        let mut items = Vec::with_capacity(cell.features.len());
        let mut coverage = Vec::new();
        let mut contours = Vec::new();
        let mut index: HashMap<FeatureId, usize> = HashMap::new();
        let mut slave_ids = Vec::new();
        for f in cell.features {
            let geom = match &f.geometry {
                Geometry::None => Geom::None,
                Geometry::Point(p) => Geom::Point(project(p)),
                Geometry::Soundings(s) => {
                    Geom::Soundings(s.iter().map(|(p, d)| (project(p), *d as f32)).collect())
                }
                Geometry::Lines(l) => Geom::Lines(projected(l)),
                Geometry::Area { rings, outline } => Geom::Area {
                    rings: projected(rings),
                    outline: projected(outline),
                },
            };
            let mut bbox = Rect::EMPTY;
            match &geom {
                Geom::None => {}
                Geom::Point(p) => bbox.add(*p),
                Geom::Soundings(s) => s.iter().for_each(|(p, _)| bbox.add(*p)),
                Geom::Lines(l) => l.iter().flatten().for_each(|p| bbox.add(*p)),
                Geom::Area { rings, .. } => rings.iter().flatten().for_each(|p| bbox.add(*p)),
            }
            let item = Item {
                class: f.class,
                id: f.id,
                attrs: f.attrs,
                geom,
                bbox,
                scamin: f64::INFINITY,
                slaves: Vec::new(),
                master: None,
            };
            let scamin = item
                .num(SCAMIN)
                .filter(|s| *s > 0.0)
                .unwrap_or(f64::INFINITY);
            if item.class == M_COVR
                && item.first(CATCOV) == Some(1)
                && let Geom::Area { rings, .. } = &item.geom
            {
                coverage.extend(rings.iter().cloned());
            }
            if item.class == DEPCNT
                && let Some(v) = item.num(VALDCO)
            {
                contours.push(v);
            }
            index.insert(item.id, items.len());
            slave_ids.push(f.slaves);
            items.push(Item { scamin, ..item });
        }
        for (i, ids) in slave_ids.into_iter().enumerate() {
            for id in ids {
                if let Some(&j) = index.get(&id) {
                    items[i].slaves.push(j);
                    items[j].master = Some(i);
                }
            }
        }
        contours.sort_by(f64::total_cmp);
        contours.dedup();
        let mut bounds = Rect::EMPTY;
        coverage.iter().flatten().for_each(|p| bounds.add(*p));
        if bounds.is_empty() {
            items.iter().for_each(|i| {
                if !i.bbox.is_empty() {
                    bounds.union(&i.bbox)
                }
            });
        }
        Chart {
            name: cell.name,
            scale: cell.scale,
            edition: cell.edition,
            update: cell.update,
            issued: cell.issued,
            bounds,
            coverage,
            items,
            contours,
        }
    }

    /// The contour that stands for the safety contour in this cell: the
    /// shallowest one at least as deep as wanted. Contours in NOAA cells
    /// are feet converted to metres and rounded, so 12 ft may be 3.6 m.
    pub fn safety_contour(&self, wanted: f64) -> f64 {
        self.contours
            .iter()
            .copied()
            .find(|&c| c >= wanted - 0.1)
            .unwrap_or(wanted)
    }
}
