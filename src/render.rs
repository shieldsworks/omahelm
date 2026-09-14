//! Drawing a chart tile: the cells that cover it, coarse first so finer
//! charts paint over them, each drawn as areas, then lines, then symbols,
//! then names.
//!
//! The symbols follow a paper chart and the spirit of IHO S-52 without
//! copying its symbol library: a green can, a red nun, a light's magenta
//! flare, soundings in the depth units you steer by.

use crate::chart::{Chart, Geom, Item, P};
use crate::geo::{self, Rect};
use crate::library::{Entry, Library};
use crate::marks;
use crate::s57::names::*;
use crate::style::{Palette, Rgb, Settings};
use crate::text::Font;
use tiny_skia::{
    FillRule, LineCap, LineJoin, Paint, Path, PathBuilder, Pixmap, Stroke, StrokeDash, Transform,
};

/// Logical pixels per tile side.
pub const TILE: f64 = 256.0;
/// How far outside a tile a symbol's anchor may be and still reach in.
const SYMBOL_REACH: f64 = 40.0;
/// The same for names, which run wider.
const LABEL_REACH: f64 = 200.0;
/// Bumped whenever drawing changes, so cached tiles are redrawn.
pub const VERSION: u32 = 4;
/// The smallest scales, as denominators, that still show each kind of
/// text. Zoomed out further, a chart shows its marks without the words.
const AID_LABELS: f64 = 45_000.0;
const LIGHT_TEXT: f64 = 30_000.0;
const MINOR_TEXT: f64 = 60_000.0;

pub struct Style {
    pub palette: Palette,
    pub settings: Settings,
}

impl Style {
    pub fn key(&self) -> String {
        format!("{VERSION}|{}|{}", self.palette.key(), self.settings.key())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TileKey {
    pub z: u32,
    pub x: u32,
    pub y: u32,
    /// Device pixels per logical pixel, 1 to 4.
    pub scale: u32,
}

impl TileKey {
    pub fn valid(&self) -> bool {
        self.z <= 20
            && self.x < (1 << self.z)
            && self.y < (1 << self.z)
            && (1..=4).contains(&self.scale)
    }
}

/// The cells a tile draws, coarse first. A cell is drawn while the view is
/// at most four times smaller in scale than it was compiled for, so a
/// harbour chart isn't drawn across a whole coast.
pub fn cells_for<'a>(lib: &'a Library, rect: &Rect, display: f64) -> Vec<&'a Entry> {
    let mut all = lib.around(rect);
    all.sort_by(|a, b| b.scale.cmp(&a.scale).then(a.name.cmp(&b.name)));
    let usable: Vec<&Entry> = all
        .iter()
        .copied()
        .filter(|e| display <= f64::from(e.scale) * 4.0)
        .collect();
    if usable.is_empty() {
        // Better a chart drawn small than no chart.
        all.into_iter().take(16).collect()
    } else {
        usable
    }
}

pub fn render(
    lib: &Library,
    style: &Style,
    font: Option<&Font>,
    key: TileKey,
) -> Result<Pixmap, String> {
    let s = key.scale as f32;
    let size = (TILE as f32 * s).round() as u32;
    let mut pm = Pixmap::new(size, size).ok_or("tile too large")?;
    let pal = &style.palette;
    let bg = pal.nodata;
    pm.fill(tiny_skia::Color::from_rgba8(bg.0, bg.1, bg.2, 255));
    let view = Rect::tile(key.z, key.x, key.y);
    let world = f64::from(key.z).exp2() * TILE;
    let lat = geo::lon_lat(view.center()).1;
    let display = geo::scale_denominator(f64::from(key.z), lat);
    let mut canvas = Canvas {
        pm,
        ox: view.x0,
        oy: view.y0,
        k: world * f64::from(s),
        s,
        size: size as f32,
        pal,
        set: &style.settings,
        font,
        view,
        display,
        unit: 1.0 / world,
    };
    for entry in cells_for(lib, &view.expand(canvas.unit * LABEL_REACH), display) {
        match lib.load(entry) {
            Ok(chart) => canvas.chart(&chart),
            Err(e) => eprintln!("omahelm: {}: {e}", entry.name),
        }
    }
    Ok(canvas.pm)
}

/// A tile as PNG, without the alpha channel it never uses.
pub fn png(pm: &Pixmap) -> Result<Vec<u8>, String> {
    let mut rgb = Vec::with_capacity(pm.data().len() / 4 * 3);
    for px in pm.data().as_chunks::<4>().0 {
        rgb.extend_from_slice(&px[..3]);
    }
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, pm.width(), pm.height());
        enc.set_color(png::ColorType::Rgb);
        enc.set_depth(png::BitDepth::Eight);
        enc.set_compression(png::Compression::Fast);
        let mut w = enc.write_header().map_err(|e| e.to_string())?;
        w.write_image_data(&rgb).map_err(|e| e.to_string())?;
    }
    Ok(out)
}

#[derive(Clone, Copy)]
enum Anchor {
    Left,
    Center,
}

struct Canvas<'a> {
    pm: Pixmap,
    ox: f64,
    oy: f64,
    /// Device pixels per Mercator unit.
    k: f64,
    /// Device pixels per logical pixel.
    s: f32,
    size: f32,
    pal: &'a Palette,
    set: &'a Settings,
    font: Option<&'a Font>,
    view: Rect,
    /// The scale on show, as its denominator.
    display: f64,
    /// Mercator units per logical pixel.
    unit: f64,
}

type Xy = (f32, f32);

/// Clips a polygon to a rectangle (Sutherland–Hodgman). Holes survive,
/// since each ring is clipped alone and the fill is even-odd.
fn clip(ring: &[(f64, f64)], x0: f64, y0: f64, x1: f64, y1: f64) -> Vec<(f64, f64)> {
    let mut out = ring.to_vec();
    for edge in 0..4 {
        let input = std::mem::take(&mut out);
        let Some(&last) = input.last() else { break };
        let inside = |p: (f64, f64)| match edge {
            0 => p.0 >= x0,
            1 => p.0 <= x1,
            2 => p.1 >= y0,
            _ => p.1 <= y1,
        };
        let cross = |a: (f64, f64), b: (f64, f64)| {
            let t = match edge {
                0 => (x0 - a.0) / (b.0 - a.0),
                1 => (x1 - a.0) / (b.0 - a.0),
                2 => (y0 - a.1) / (b.1 - a.1),
                _ => (y1 - a.1) / (b.1 - a.1),
            };
            (a.0 + t * (b.0 - a.0), a.1 + t * (b.1 - a.1))
        };
        let mut prev = last;
        for &cur in &input {
            match (inside(prev), inside(cur)) {
                (true, true) => out.push(cur),
                (true, false) => out.push(cross(prev, cur)),
                (false, true) => {
                    out.push(cross(prev, cur));
                    out.push(cur);
                }
                (false, false) => {}
            }
            prev = cur;
        }
    }
    out
}

/// Harbour-chart soundings show at twice their SCAMIN: NOAA hides them
/// until 1:22,000, and a harbour without depths isn't much of a chart.
/// Offshore soundings are dense enough already.
fn sounding_scamin(scamin: f64) -> f64 {
    if scamin < 50_000.0 {
        scamin * 2.0
    } else {
        scamin
    }
}

/// The order areas fill in; None for classes that aren't filled.
fn area_rank(class: u16) -> Option<u8> {
    Some(match class {
        DEPARE | DRGARE | UNSARE => 0,
        LNDARE => 1,
        LAKARE | RIVERS | CANALS | DOCARE => 2,
        BUAARE => 3,
        TSEZNE => 4,
        OBSTRN => 5,
        PONTON | HULKES | FLODOC | DRYDOC | CAUSWY | DAMCON | DYKCON | SLCONS | BRIDGE | GATCON
        | OFSPLF | RUNWAY => 6,
        BUISGL | SILTNK => 7,
        _ => return None,
    })
}

/// Areas whose boundary is a magenta dashed line: places with rules.
fn regulated(class: u16) -> bool {
    matches!(
        class,
        RESARE
            | ACHARE
            | CTNARE
            | MIPARE
            | SPLARE
            | PRCARE
            | DMPGRD
            | CBLARE
            | PIPARE
            | ISTZNE
            | OSPARE
            | FSHFAC
            | MARCUL
            | PRDARE
            | FERYRT
            | DWRTPT
            | TSSCRS
            | TSSRON
    )
}

impl Canvas<'_> {
    fn px(&self, p: P) -> Xy {
        (
            ((p[0] - self.ox) * self.k) as f32,
            ((p[1] - self.oy) * self.k) as f32,
        )
    }

    fn reach(&self, logical: f64) -> Rect {
        self.view.expand(self.unit * logical)
    }

    fn visible(&self, item: &Item, logical: f64) -> bool {
        let scamin = if item.class == SOUNDG {
            sounding_scamin(item.scamin)
        } else {
            item.scamin
        };
        self.display <= scamin
            && !item.bbox.is_empty()
            && item.bbox.intersects(&self.reach(logical))
    }

    fn paint(c: Rgb, alpha: u8, aa: bool) -> Paint<'static> {
        let mut p = Paint::default();
        p.set_color_rgba8(c.0, c.1, c.2, alpha);
        p.anti_alias = aa;
        p
    }

    fn fill(&mut self, path: &Path, c: Rgb, alpha: u8, aa: bool) {
        self.pm.fill_path(
            path,
            &Self::paint(c, alpha, aa),
            FillRule::EvenOdd,
            Transform::identity(),
            None,
        );
    }

    fn fill_at(&mut self, path: &Path, c: Rgb, t: Transform) {
        self.pm
            .fill_path(path, &Self::paint(c, 255, true), FillRule::Winding, t, None);
    }

    fn stroke(&mut self, path: &Path, c: Rgb, width: f32, dash: &[f32]) {
        self.stroke_alpha(path, c, 255, width, dash, Transform::identity());
    }

    fn stroke_alpha(
        &mut self,
        path: &Path,
        c: Rgb,
        alpha: u8,
        width: f32,
        dash: &[f32],
        t: Transform,
    ) {
        let mut stroke = Stroke {
            width: width * self.s,
            line_cap: LineCap::Round,
            line_join: LineJoin::Round,
            ..Default::default()
        };
        if !dash.is_empty() {
            stroke.dash = StrokeDash::new(dash.iter().map(|d| d * self.s).collect(), 0.0);
            stroke.line_cap = LineCap::Butt;
        }
        self.pm
            .stroke_path(path, &Self::paint(c, alpha, true), &stroke, t, None);
    }

    fn ring_path(&self, rings: &[Vec<P>]) -> Option<Path> {
        let pad = f64::from(4.0 * self.s);
        let (lo, hi) = (-pad, f64::from(self.size) + pad);
        let mut pb = PathBuilder::new();
        for ring in rings {
            let pts: Vec<(f64, f64)> = ring
                .iter()
                .map(|p| ((p[0] - self.ox) * self.k, (p[1] - self.oy) * self.k))
                .collect();
            let c = clip(&pts, lo, lo, hi, hi);
            if c.len() < 3 {
                continue;
            }
            pb.move_to(c[0].0 as f32, c[0].1 as f32);
            for q in &c[1..] {
                pb.line_to(q.0 as f32, q.1 as f32);
            }
            pb.close();
        }
        pb.finish()
    }

    /// Polylines, without the segments far outside the tile, which keeps
    /// coordinates small at high zoom.
    fn line_path(&self, lines: &[Vec<P>]) -> Option<Path> {
        let pad = f64::from(16.0 * self.s);
        let (lo, hi) = (-pad, f64::from(self.size) + pad);
        let mut pb = PathBuilder::new();
        for line in lines {
            let mut pen = false;
            for w in line.windows(2) {
                let a = ((w[0][0] - self.ox) * self.k, (w[0][1] - self.oy) * self.k);
                let b = ((w[1][0] - self.ox) * self.k, (w[1][1] - self.oy) * self.k);
                if a.0.max(b.0) < lo || a.0.min(b.0) > hi || a.1.max(b.1) < lo || a.1.min(b.1) > hi
                {
                    pen = false;
                    continue;
                }
                if !pen {
                    pb.move_to(a.0 as f32, a.1 as f32);
                    pen = true;
                }
                pb.line_to(b.0 as f32, b.1 as f32);
            }
        }
        pb.finish()
    }

    fn stroke_lines(&mut self, lines: &[Vec<P>], c: Rgb, width: f32, dash: &[f32]) {
        if let Some(path) = self.line_path(lines) {
            self.stroke(&path, c, width, dash);
        }
    }

    fn stroke_lines_alpha(
        &mut self,
        lines: &[Vec<P>],
        c: Rgb,
        alpha: u8,
        width: f32,
        dash: &[f32],
    ) {
        if let Some(path) = self.line_path(lines) {
            self.stroke_alpha(&path, c, alpha, width, dash, Transform::identity());
        }
    }

    /// A polygon in logical pixels about a point.
    fn shape(&self, at: Xy, pts: &[Xy]) -> Option<Path> {
        let mut pb = PathBuilder::new();
        let (first, rest) = pts.split_first()?;
        pb.move_to(at.0 + first.0 * self.s, at.1 + first.1 * self.s);
        for p in rest {
            pb.line_to(at.0 + p.0 * self.s, at.1 + p.1 * self.s);
        }
        pb.close();
        pb.finish()
    }

    /// Line segments in logical pixels about a point.
    fn segments(&self, at: Xy, segs: &[(Xy, Xy)]) -> Option<Path> {
        let mut pb = PathBuilder::new();
        for (a, b) in segs {
            pb.move_to(at.0 + a.0 * self.s, at.1 + a.1 * self.s);
            pb.line_to(at.0 + b.0 * self.s, at.1 + b.1 * self.s);
        }
        pb.finish()
    }

    fn circle(&self, at: Xy, r: f32) -> Option<Path> {
        PathBuilder::from_circle(at.0, at.1, r * self.s)
    }

    #[allow(clippy::too_many_arguments)]
    fn text(
        &mut self,
        text: &str,
        at: Xy,
        size: f32,
        c: Rgb,
        anchor: Anchor,
        italic: bool,
        halo: bool,
    ) -> f32 {
        let Some(font) = self.font else { return 0.0 };
        if text.is_empty() {
            return 0.0;
        }
        let px = size * self.s;
        let w = font.width(text, px);
        let x = match anchor {
            Anchor::Left => at.0,
            Anchor::Center => at.0 - w / 2.0,
        };
        // `at` is the middle of the capitals.
        let y = at.1 + font.cap() * px / 2.0;
        let Some(path) = font.path(text, px, x, y, italic) else {
            return w;
        };
        if halo {
            let ground = self.pal.deep;
            self.stroke_alpha(&path, ground, 210, 2.6, &[], Transform::identity());
        }
        self.fill(&path, c, 255, true);
        w
    }

    fn text_width(&self, text: &str, size: f32) -> f32 {
        self.font.map_or(0.0, |f| f.width(text, size * self.s))
    }

    fn chart(&mut self, chart: &Chart) {
        let safety = chart.safety_contour(self.set.safety_contour);
        let mut areas: Vec<(u8, &Item)> = chart
            .items
            .iter()
            .filter(|i| matches!(i.geom, Geom::Area { .. }))
            .filter_map(|i| area_rank(i.class).map(|r| (r, i)))
            .filter(|(_, i)| self.visible(i, 0.0))
            .collect();
        areas.sort_by_key(|(r, _)| *r);
        for (_, item) in &areas {
            self.area(item, safety);
        }
        for item in &chart.items {
            if self.visible(item, 2.0) {
                self.lines(item, safety);
            }
        }
        for item in &chart.items {
            if self.visible(item, SYMBOL_REACH) {
                self.point(chart, item, safety);
            }
        }
        for item in &chart.items {
            if self.visible(item, LABEL_REACH) {
                self.name(item);
            }
        }
    }

    fn depth_colour(&self, item: &Item, safety: f64) -> Rgb {
        let pal = self.pal;
        let d1 = item.num(DRVAL1).unwrap_or(-1.0);
        let d2 = item.num(DRVAL2).unwrap_or(d1 + 0.01);
        if d1 < 0.0 && d2 <= 0.0 {
            return pal.drying;
        }
        // S-52's four shades, by the shallow end of the band. Contours are
        // rounded conversions from feet, so they're matched loosely.
        if d1 >= self.set.deep_contour.max(safety) - 0.1 {
            pal.deep
        } else if d1 >= safety - 1e-6 {
            pal.medium_deep
        } else if d1 >= self.set.shallow_contour - 0.1 {
            pal.medium_shallow
        } else {
            pal.very_shallow
        }
    }

    fn area(&mut self, item: &Item, safety: f64) {
        let Geom::Area { rings, .. } = &item.geom else {
            return;
        };
        let pal = self.pal;
        let (c, alpha, aa) = match item.class {
            DEPARE | DRGARE => (self.depth_colour(item, safety), 255, false),
            UNSARE => (pal.nodata, 255, false),
            LNDARE => (pal.land, 255, false),
            LAKARE | RIVERS | CANALS | DOCARE => (pal.medium_shallow, 255, true),
            BUAARE => (pal.built, 255, true),
            TSEZNE => (pal.magenta, 36, true),
            OBSTRN => (pal.very_shallow, 255, true),
            BUISGL | SILTNK => (pal.structure, 255, true),
            _ => (pal.structure.mix(pal.land, 0.45), 255, true),
        };
        if let Some(path) = self.ring_path(rings) {
            self.fill(&path, c, alpha, aa);
        }
    }

    fn lines(&mut self, item: &Item, safety: f64) {
        let pal = self.pal;
        match &item.geom {
            Geom::Lines(l) => match item.class {
                DEPCNT => {
                    if (item.num(VALDCO).unwrap_or(-1.0) - safety).abs() < 1e-6 {
                        self.stroke_lines(l, pal.ink, 1.3, &[]);
                    } else {
                        self.stroke_lines(l, pal.contour, 0.6, &[]);
                    }
                }
                COALNE | LNDARE => self.stroke_lines(l, pal.ink, 1.0, &[]),
                SLCONS | CAUSWY | DYKCON | DAMCON | GATCON => {
                    self.stroke_lines(l, pal.ink, 1.4, &[])
                }
                BRIDGE => self.stroke_lines(l, pal.structure, 3.0, &[]),
                CBLSUB => self.stroke_lines(l, pal.magenta, 0.9, &[3.0, 3.0]),
                CBLOHD | PIPOHD => self.stroke_lines(l, pal.faint, 1.0, &[6.0, 3.0]),
                PIPSOL => self.stroke_lines(l, pal.magenta, 0.9, &[8.0, 3.0]),
                NAVLNE => self.stroke_lines(l, pal.ink, 0.8, &[8.0, 4.0]),
                RECTRC => self.stroke_lines(l, pal.ink, 1.0, &[]),
                TSSBND => self.stroke_lines_alpha(l, pal.magenta, 200, 1.6, &[10.0, 5.0]),
                TSELNE => self.stroke_lines_alpha(l, pal.magenta, 70, 6.0, &[]),
                FERYRT => self.stroke_lines(l, pal.magenta, 0.9, &[10.0, 3.0, 2.0, 3.0]),
                RAILWY | ROADWY => self.stroke_lines(l, pal.faint, 0.8, &[]),
                RIVERS | CANALS => self.stroke_lines(l, pal.contour, 1.0, &[]),
                FNCLNE | SLOTOP | CONVYR => self.stroke_lines(l, pal.faint, 0.6, &[]),
                DWRTCL | RCRTCL => self.stroke_lines(l, pal.magenta, 1.0, &[8.0, 4.0]),
                _ => {}
            },
            Geom::Area { outline, .. } => match item.class {
                c if regulated(c) => {
                    self.stroke_lines_alpha(outline, pal.magenta, 150, 0.9, &[7.0, 4.0])
                }
                FAIRWY => self.stroke_lines(outline, pal.faint, 0.8, &[8.0, 4.0]),
                DRGARE => self.stroke_lines(outline, pal.faint, 0.8, &[4.0, 3.0]),
                UNSARE => self.stroke_lines(outline, pal.faint, 0.6, &[]),
                OBSTRN => self.stroke_lines(outline, pal.ink, 1.0, &[1.2, 2.2]),
                PONTON | HULKES | FLODOC | DRYDOC | BUISGL | SILTNK | CAUSWY | DAMCON | DYKCON
                | SLCONS | BRIDGE | GATCON | OFSPLF => {
                    self.stroke_lines(outline, pal.ink, 0.8, &[])
                }
                LAKARE | CANALS | RIVERS | DOCARE => {
                    self.stroke_lines(outline, pal.contour, 0.8, &[])
                }
                _ => {}
            },
            _ => {}
        }
    }

    fn point(&mut self, chart: &Chart, item: &Item, safety: f64) {
        match item.class {
            SOUNDG => self.soundings(item),
            BOYLAT | BOYCAR | BOYISD | BOYSAW | BOYSPP | BOYINB => self.buoy(chart, item),
            BCNLAT | BCNCAR | BCNISD | BCNSAW | BCNSPP => self.beacon(chart, item),
            LITFLT | LITVES => self.lightship(chart, item),
            LIGHTS => self.light(item),
            UWTROC => self.rock(chart, item, safety),
            WRECKS => self.wreck(chart, item, safety),
            OBSTRN => self.obstruction(chart, item, safety),
            LNDMRK => self.landmark(item),
            PILPNT => {
                if let Geom::Point(p) = item.geom
                    && let Some(c) = self.circle(self.px(p), 1.4)
                {
                    self.fill(&c, self.pal.ink, 255, true);
                }
            }
            MORFAC => self.mooring(item),
            RDOSTA | RTPBCN | RADSTA => {
                if let Geom::Point(p) = item.geom
                    && let Some(c) = self.circle(self.px(p), 7.0)
                {
                    self.stroke(&c, self.pal.magenta, 0.8, &[]);
                }
            }
            SBDARE => {
                if let Geom::Point(p) = item.geom {
                    let t = marks::bottom(item);
                    self.text(
                        &t,
                        self.px(p),
                        8.5,
                        self.pal.faint,
                        Anchor::Center,
                        true,
                        false,
                    );
                }
            }
            ACHARE | ACHBRT => self.anchorage(item),
            TSSLPT => self.lane(item),
            BRIDGE | CBLOHD | PIPOHD => self.clearance(item),
            _ => {}
        }
    }

    fn soundings(&mut self, item: &Item) {
        let Geom::Soundings(list) = &item.geom else {
            return;
        };
        if self.display > sounding_scamin(item.scamin) {
            return;
        }
        let reach = self.reach(12.0);
        let units = self.set.units;
        for (p, d) in list {
            if !reach.contains(*p) {
                continue;
            }
            let at = self.px(*p);
            let d = f64::from(*d);
            let c = if d <= self.set.safety_depth {
                self.pal.ink
            } else {
                self.pal.faint
            };
            let (main, sub) = marks::depth(d, units);
            let w_main = self.text_width(&main, 10.0);
            let w_sub = sub.as_deref().map_or(0.0, |s| self.text_width(s, 7.0));
            let x0 = at.0 - (w_main + w_sub) / 2.0;
            self.text(&main, (x0, at.1), 10.0, c, Anchor::Left, false, false);
            if let Some(sub) = sub {
                self.text(
                    &sub,
                    (x0 + w_main, at.1 + 2.5 * self.s),
                    7.0,
                    c,
                    Anchor::Left,
                    false,
                    false,
                );
            }
            if d < 0.0 {
                // Drying heights are underlined, as on a paper chart.
                let y = at.1 + 5.5 * self.s;
                if let Some(path) =
                    self.segments((0.0, 0.0), &[((x0, y), (x0 + w_main + w_sub, y))])
                {
                    let s = self.s;
                    self.s = 1.0;
                    self.stroke(&path, c, 0.8 * s, &[]);
                    self.s = s;
                }
            }
        }
    }

    fn aid_colour(&self, code: u32) -> Rgb {
        let pal = self.pal;
        match code {
            1 => pal.white,
            2 => pal.black,
            3 => pal.red,
            4 => pal.green,
            5 => pal.blue,
            6 => pal.yellow,
            9 | 11 => pal.orange,
            10 | 12 | 13 => pal.magenta,
            8 => pal.built,
            _ => pal.faint,
        }
    }

    /// Fills a symbol in its colours: horizontal bands, or vertical
    /// stripes when the pattern says so.
    fn painted(&mut self, at: Xy, body: &[Xy], colours: &[u32], pattern: Option<u32>) {
        let s = self.s;
        let pts: Vec<(f64, f64)> = body
            .iter()
            .map(|(x, y)| (f64::from(at.0 + x * s), f64::from(at.1 + y * s)))
            .collect();
        let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
        for p in &pts {
            x0 = x0.min(p.0);
            y0 = y0.min(p.1);
            x1 = x1.max(p.0);
            y1 = y1.max(p.1);
        }
        let n = colours.len().max(1);
        for i in 0..n {
            let c = colours
                .get(i)
                .map_or(self.pal.faint, |&c| self.aid_colour(c));
            let (f0, f1) = (i as f64 / n as f64, (i + 1) as f64 / n as f64);
            let part = if pattern == Some(2) {
                clip(
                    &pts,
                    x0 + (x1 - x0) * f0,
                    y0 - 1.0,
                    x0 + (x1 - x0) * f1,
                    y1 + 1.0,
                )
            } else {
                clip(
                    &pts,
                    x0 - 1.0,
                    y0 + (y1 - y0) * f0,
                    x1 + 1.0,
                    y0 + (y1 - y0) * f1,
                )
            };
            if part.len() < 3 {
                continue;
            }
            let mut pb = PathBuilder::new();
            pb.move_to(part[0].0 as f32, part[0].1 as f32);
            for q in &part[1..] {
                pb.line_to(q.0 as f32, q.1 as f32);
            }
            pb.close();
            if let Some(path) = pb.finish() {
                self.fill(&path, c, 255, true);
            }
        }
        if let Some(path) = self.shape(at, body) {
            self.stroke(&path, self.pal.ink, 0.9, &[]);
        }
    }

    fn buoy(&mut self, chart: &Chart, item: &Item) {
        let Geom::Point(p) = item.geom else { return };
        let at = self.px(p);
        let body: Vec<Xy> = match item.first(BOYSHP).unwrap_or(4) {
            1 => vec![(-4.5, -2.5), (4.5, -2.5), (0.0, -13.0)],
            2 => vec![(-4.0, -2.5), (4.0, -2.5), (4.0, -11.5), (-4.0, -11.5)],
            3 => (0..16)
                .map(|i| {
                    let a = i as f32 * std::f32::consts::TAU / 16.0;
                    (5.0 * a.cos(), -7.5 + 5.0 * a.sin())
                })
                .collect(),
            5 => vec![(-1.5, -2.5), (1.5, -2.5), (1.5, -15.0), (-1.5, -15.0)],
            6 => vec![
                (-6.0, -3.0),
                (6.0, -3.0),
                (6.5, -5.5),
                (6.0, -8.0),
                (-6.0, -8.0),
                (-6.5, -5.5),
            ],
            7 => vec![(-7.0, -2.5), (7.0, -2.5), (5.0, -9.5), (-5.0, -9.5)],
            _ => vec![(-4.5, -2.5), (4.5, -2.5), (1.8, -14.0), (-1.8, -14.0)],
        };
        self.painted(at, &body, &item.list(COLOUR), item.first(COLPAT));
        if let Some(c) = self.circle(at, 1.6) {
            self.stroke(&c, self.pal.ink, 0.8, &[]);
        }
        self.aid_text(chart, item, at);
    }

    fn beacon(&mut self, chart: &Chart, item: &Item) {
        let Geom::Point(p) = item.geom else { return };
        let at = self.px(p);
        let colours = item.list(COLOUR);
        if let Some(stem) = self.segments(at, &[((0.0, 0.0), (0.0, -8.0))]) {
            self.stroke(&stem, self.pal.ink, 1.2, &[]);
        }
        // US daymarks: green squares to port, red triangles to starboard.
        let top: Vec<Xy> = match colours.first() {
            Some(4) => vec![(-3.8, -8.0), (3.8, -8.0), (3.8, -15.6), (-3.8, -15.6)],
            Some(3) => vec![(-4.4, -8.0), (4.4, -8.0), (0.0, -16.0)],
            _ => vec![(0.0, -7.5), (4.2, -11.8), (0.0, -16.1), (-4.2, -11.8)],
        };
        self.painted(at, &top, &colours, item.first(COLPAT));
        if let Some(c) = self.circle(at, 1.6) {
            self.stroke(&c, self.pal.ink, 0.8, &[]);
        }
        self.aid_text(chart, item, at);
    }

    fn lightship(&mut self, chart: &Chart, item: &Item) {
        let Geom::Point(p) = item.geom else { return };
        let at = self.px(p);
        let hull = [(-8.0, -2.0), (8.0, -2.0), (5.5, 2.5), (-5.5, 2.5)];
        self.painted(at, &hull, &item.list(COLOUR), None);
        if let Some(mast) = self.segments(at, &[((0.0, -2.0), (0.0, -11.0))]) {
            self.stroke(&mast, self.pal.ink, 1.2, &[]);
        }
        self.aid_text(chart, item, at);
    }

    /// A buoy or beacon's name and, on the line below, its light.
    fn aid_text(&mut self, chart: &Chart, item: &Item, at: Xy) {
        if self.display > AID_LABELS {
            return;
        }
        let s = self.s;
        let label = marks::aid_label(item);
        let ink = self.pal.ink;
        self.text(
            &label,
            (at.0 + 7.0 * s, at.1 - 9.0 * s),
            9.0,
            ink,
            Anchor::Left,
            false,
            true,
        );
        let light = item
            .slaves
            .iter()
            .map(|&j| &chart.items[j])
            .find(|l| l.class == LIGHTS)
            .map(marks::light);
        if let Some(light) = light.filter(|l| !l.is_empty() && self.display <= LIGHT_TEXT) {
            self.text(
                &light,
                (at.0 + 10.0 * s, at.1 + 1.5 * s),
                8.0,
                ink,
                Anchor::Left,
                false,
                true,
            );
        }
    }

    fn light_colour(&self, item: &Item) -> Rgb {
        match item.first(COLOUR) {
            Some(3) => self.pal.red,
            Some(4) => self.pal.green,
            Some(1 | 6) | None => self.pal.yellow,
            Some(c) => self.aid_colour(c),
        }
    }

    fn light(&mut self, item: &Item) {
        let Geom::Point(p) = item.geom else { return };
        let at = self.px(p);
        let c = self.light_colour(item);
        match (item.num(SECTR1), item.num(SECTR2)) {
            (Some(a), Some(b)) => self.sector(at, a, b, c),
            _ => {
                // The flare: a teardrop with its point on the light.
                let drop = [
                    (0.0, 0.0),
                    (-2.6, -7.5),
                    (-2.3, -10.4),
                    (0.0, -12.0),
                    (2.3, -10.4),
                    (2.6, -7.5),
                ];
                if let Some(path) = self.shape((0.0, 0.0), &drop) {
                    let t = Transform::from_rotate(135.0).post_translate(at.0, at.1);
                    self.fill_at(&path, c, t);
                    self.stroke_alpha(&path, self.pal.ink, 160, 0.5, &[], t);
                }
            }
        }
        if item.master.is_none() && self.display <= LIGHT_TEXT {
            let d = marks::light(item);
            let s = self.s;
            self.text(
                &d,
                (at.0 + 9.0 * s, at.1 + 6.0 * s),
                8.0,
                self.pal.ink,
                Anchor::Left,
                false,
                true,
            );
        }
    }

    /// A sector light: an arc in the light's colour across the bearings
    /// it shows on. SECTR1 and SECTR2 are taken from seaward, so the arc
    /// runs from the light the other way.
    fn sector(&mut self, at: Xy, from: f64, to: f64, c: Rgb) {
        let a1 = (from + 180.0).rem_euclid(360.0);
        let mut a2 = (to + 180.0).rem_euclid(360.0);
        if a2 <= a1 {
            a2 += 360.0;
        }
        let s = self.s;
        let dir = |deg: f64, r: f32| -> Xy {
            let t = deg.to_radians();
            (at.0 + r * s * t.sin() as f32, at.1 - r * s * t.cos() as f32)
        };
        let mut pb = PathBuilder::new();
        let steps = ((a2 - a1) / 3.0).ceil().max(2.0) as usize;
        for i in 0..=steps {
            let (x, y) = dir(a1 + (a2 - a1) * i as f64 / steps as f64, 20.0);
            if i == 0 {
                pb.move_to(x, y);
            } else {
                pb.line_to(x, y);
            }
        }
        if let Some(arc) = pb.finish() {
            self.stroke(&arc, self.pal.ink, 4.2, &[]);
            self.stroke(&arc, c, 3.0, &[]);
        }
        let mut legs = PathBuilder::new();
        for a in [a1, a2] {
            let (x, y) = dir(a, 34.0);
            legs.move_to(at.0, at.1);
            legs.line_to(x, y);
        }
        if let Some(legs) = legs.finish() {
            self.stroke(&legs, self.pal.faint, 0.7, &[4.0, 3.0]);
        }
    }

    /// Whether a hazard lies shallower than the safety contour in water
    /// that is otherwise deeper: an isolated danger.
    fn isolated(&self, chart: &Chart, item: &Item, depth: Option<f64>, safety: f64) -> bool {
        let shallow = match depth {
            Some(d) => d < safety,
            None => matches!(item.first(WATLEV), Some(3..=5) | None),
        };
        let Some(p) = item.anchor().filter(|_| shallow) else {
            return false;
        };
        chart.items.iter().any(|a| {
            a.class == DEPARE
                && a.bbox.contains(p)
                && a.num(DRVAL1).unwrap_or(-1.0) >= safety - 1e-6
                && matches!(&a.geom, Geom::Area { rings, .. } if geo::inside(rings, p))
        })
    }

    fn danger(&mut self, at: Xy) {
        if let Some(c) = self.circle(at, 6.5) {
            self.stroke(&c, self.pal.magenta, 1.4, &[]);
        }
        if let Some(x) = self.segments(
            at,
            &[((-3.2, -3.2), (3.2, 3.2)), ((-3.2, 3.2), (3.2, -3.2))],
        ) {
            self.stroke(&x, self.pal.magenta, 1.2, &[]);
        }
    }

    fn hazard_depth(&mut self, at: Xy, depth: Option<f64>) {
        if let Some(d) = depth.filter(|_| self.display <= MINOR_TEXT) {
            let depth = marks::depth_number(d, self.set.units);
            let s = self.s;
            self.text(
                &depth,
                (at.0 + 7.5 * s, at.1 + 4.0 * s),
                8.0,
                self.pal.ink,
                Anchor::Left,
                false,
                false,
            );
        }
    }

    fn rock(&mut self, chart: &Chart, item: &Item, safety: f64) {
        let Geom::Point(p) = item.geom else { return };
        let at = self.px(p);
        let depth = item.num(VALSOU);
        if self.isolated(chart, item, depth, safety) {
            self.danger(at);
            self.hazard_depth(at, depth);
            return;
        }
        let ink = self.pal.ink;
        let plus = [((-3.5, 0.0), (3.5, 0.0)), ((0.0, -3.5), (0.0, 3.5))];
        match item.first(WATLEV) {
            Some(4) => {
                let star = [
                    ((-3.5, 0.0), (3.5, 0.0)),
                    ((0.0, -3.5), (0.0, 3.5)),
                    ((-2.5, -2.5), (2.5, 2.5)),
                    ((-2.5, 2.5), (2.5, -2.5)),
                ];
                if let Some(path) = self.segments(at, &star) {
                    self.stroke(&path, ink, 1.0, &[]);
                }
            }
            Some(1 | 2) => {
                if let Some(c) = self.circle(at, 2.2) {
                    self.fill(&c, ink, 255, true);
                }
            }
            wl => {
                if let Some(path) = self.segments(at, &plus) {
                    self.stroke(&path, ink, 1.0, &[]);
                }
                if wl == Some(5) {
                    for (dx, dy) in [(-2.2, -2.2), (2.2, -2.2), (-2.2, 2.2), (2.2, 2.2)] {
                        if let Some(c) = self.circle((at.0 + dx * self.s, at.1 + dy * self.s), 0.7)
                        {
                            self.fill(&c, ink, 255, true);
                        }
                    }
                }
            }
        }
        self.hazard_depth(at, depth);
    }

    fn wreck(&mut self, chart: &Chart, item: &Item, safety: f64) {
        let Some(p) = item.anchor() else { return };
        let at = self.px(p);
        let depth = item.num(VALSOU);
        if self.isolated(chart, item, depth, safety) {
            self.danger(at);
            self.hazard_depth(at, depth);
            return;
        }
        let showing = item.first(CATWRK) == Some(5) || matches!(item.first(WATLEV), Some(1 | 2));
        let ink = self.pal.ink;
        if showing {
            if let Some(hull) =
                self.shape(at, &[(-6.0, -1.0), (6.0, -1.0), (4.0, 2.5), (-4.0, 2.5)])
            {
                self.fill(&hull, ink, 255, true);
            }
            if let Some(mast) = self.segments(at, &[((0.0, -1.0), (0.0, -6.0))]) {
                self.stroke(&mast, ink, 1.0, &[]);
            }
        } else {
            let c = if item.first(CATWRK) == Some(1) {
                self.pal.faint
            } else {
                ink
            };
            let bars = [
                ((-6.5, 0.0), (6.5, 0.0)),
                ((-3.5, -2.8), (-3.5, 2.8)),
                ((0.0, -2.8), (0.0, 2.8)),
                ((3.5, -2.8), (3.5, 2.8)),
            ];
            if let Some(path) = self.segments(at, &bars) {
                self.stroke(&path, c, 1.0, &[]);
            }
        }
        self.hazard_depth(at, depth);
    }

    fn obstruction(&mut self, chart: &Chart, item: &Item, safety: f64) {
        let Geom::Point(p) = item.geom else { return };
        let at = self.px(p);
        let depth = item.num(VALSOU);
        if self.isolated(chart, item, depth, safety) {
            self.danger(at);
        } else if let Some(c) = self.circle(at, 4.5) {
            self.stroke(&c, self.pal.ink, 1.0, &[1.2, 1.8]);
        }
        self.hazard_depth(at, depth);
    }

    fn landmark(&mut self, item: &Item) {
        let Geom::Point(p) = item.geom else { return };
        let at = self.px(p);
        if let Some(c) = self.circle(at, 3.4) {
            self.stroke(&c, self.pal.ink, 1.0, &[]);
        }
        if let Some(c) = self.circle(at, 1.0) {
            self.fill(&c, self.pal.ink, 255, true);
        }
        if item.first(CONVIS) == Some(1)
            && self.display <= MINOR_TEXT
            && let Some(name) = item.attr(OBJNAM)
        {
            let s = self.s;
            self.text(
                name,
                (at.0 + 6.0 * s, at.1),
                8.5,
                self.pal.ink,
                Anchor::Left,
                false,
                true,
            );
        }
    }

    fn mooring(&mut self, item: &Item) {
        let Some(p) = item.anchor() else { return };
        let at = self.px(p);
        let ink = self.pal.ink;
        match item.first(CATMOR) {
            Some(7) => {
                if let Some(c) = self.circle((at.0, at.1 - 2.5 * self.s), 2.5) {
                    self.fill(&c, self.pal.white, 255, true);
                    self.stroke(&c, ink, 0.9, &[]);
                }
            }
            _ => {
                if let Some(sq) =
                    self.shape(at, &[(-2.2, -2.2), (2.2, -2.2), (2.2, 2.2), (-2.2, 2.2)])
                {
                    self.stroke(&sq, ink, 0.9, &[]);
                }
            }
        }
    }

    /// Whether an area is at least this many logical pixels across.
    fn big(&self, item: &Item, logical: f64) -> bool {
        let w = (item.bbox.x1 - item.bbox.x0) / self.unit;
        let h = (item.bbox.y1 - item.bbox.y0) / self.unit;
        w.min(h) >= logical
    }

    fn anchorage(&mut self, item: &Item) {
        if matches!(item.geom, Geom::Area { .. }) && !self.big(item, 36.0) {
            return;
        }
        let Some(p) = item.anchor() else { return };
        let at = self.px(p);
        let m = self.pal.magenta;
        let anchor = [
            ((0.0, -6.0), (0.0, 5.0)),
            ((-3.0, -3.5), (3.0, -3.5)),
            ((-5.0, 1.5), (-2.8, 4.6)),
            ((-2.8, 4.6), (0.0, 5.0)),
            ((0.0, 5.0), (2.8, 4.6)),
            ((2.8, 4.6), (5.0, 1.5)),
        ];
        if let Some(path) = self.segments(at, &anchor) {
            self.stroke(&path, m, 1.1, &[]);
        }
        if let Some(c) = self.circle((at.0, at.1 - 7.2 * self.s), 1.3) {
            self.stroke(&c, m, 1.0, &[]);
        }
    }

    /// A traffic lane's direction, as an open arrow.
    fn lane(&mut self, item: &Item) {
        let Some(orient) = item.num(ORIENT) else {
            return;
        };
        if !self.big(item, 48.0) {
            return;
        }
        let Some(p) = item.anchor() else { return };
        let at = self.px(p);
        let arrow = [
            (-2.5, 14.0),
            (2.5, 14.0),
            (2.5, -6.0),
            (6.5, -6.0),
            (0.0, -14.0),
            (-6.5, -6.0),
            (-2.5, -6.0),
        ];
        if let Some(path) = self.shape((0.0, 0.0), &arrow) {
            let t = Transform::from_rotate(orient as f32).post_translate(at.0, at.1);
            self.stroke_alpha(&path, self.pal.magenta, 220, 1.2, &[], t);
        }
    }

    fn clearance(&mut self, item: &Item) {
        if self.display > MINOR_TEXT {
            return;
        }
        let Some(v) = item.num(VERCLR).or_else(|| item.num(VERCCL)) else {
            return;
        };
        let Some(p) = item.anchor() else { return };
        let at = self.px(p);
        let units = self.set.units;
        let t = format!(
            "clr {} {}",
            marks::trim_number(units.from_metres(v).round()),
            units.short()
        );
        self.text(
            &t,
            (at.0, at.1 + 9.0 * self.s),
            8.0,
            self.pal.ink,
            Anchor::Center,
            false,
            true,
        );
    }

    fn name(&mut self, item: &Item) {
        let pal = self.pal;
        let (size, italic, c) = match item.class {
            SEAARE => (11.0, true, pal.ink.mix(pal.contour, 0.35)),
            LNDRGN => (9.5, false, pal.faint),
            BUAARE => (9.5, false, pal.faint),
            _ => return,
        };
        let Some(name) = item.attr(OBJNAM) else {
            return;
        };
        let Some(p) = item.anchor() else { return };
        if matches!(item.geom, Geom::Area { .. }) {
            let w = f64::from(self.text_width(name, size) / self.s);
            if !self.big(item, 14.0) || (item.bbox.x1 - item.bbox.x0) / self.unit < w * 1.3 {
                return;
            }
        }
        let at = self.px(p);
        self.text(name, at, size, c, Anchor::Center, italic, true);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clip_keeps_the_inside() {
        let square = [(-10.0, -10.0), (10.0, -10.0), (10.0, 10.0), (-10.0, 10.0)];
        let c = clip(&square, 0.0, 0.0, 5.0, 5.0);
        let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
        for p in &c {
            x0 = x0.min(p.0);
            y0 = y0.min(p.1);
            x1 = x1.max(p.0);
            y1 = y1.max(p.1);
        }
        assert_eq!((x0, y0, x1, y1), (0.0, 0.0, 5.0, 5.0));
        assert!(clip(&square, 20.0, 20.0, 30.0, 30.0).is_empty());
    }

    #[test]
    fn tile_keys() {
        assert!(
            TileKey {
                z: 15,
                x: 5249,
                y: 12655,
                scale: 2
            }
            .valid()
        );
        assert!(
            !TileKey {
                z: 3,
                x: 8,
                y: 0,
                scale: 1
            }
            .valid()
        );
        assert!(
            !TileKey {
                z: 3,
                x: 0,
                y: 0,
                scale: 9
            }
            .valid()
        );
    }
}
