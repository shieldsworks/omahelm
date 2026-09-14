//! Text on the chart: glyph outlines from a system font, filled as paths,
//! so soundings and names are antialiased and can carry a halo.

use tiny_skia::{Path, PathBuilder, Transform};
use ttf_parser::{Face, GlyphId, OutlineBuilder};

pub struct Font {
    face: Face<'static>,
    units: f32,
    pub file: String,
}

/// Where fontconfig and the usual Arch packages put a font, in order of
/// preference: the Omarchy font first, so the chart reads like the desktop.
fn candidates() -> Vec<String> {
    let mut out = Vec::new();
    if let Ok(f) = std::env::var("OMAHELM_FONT") {
        out.push(f);
    }
    for pattern in ["monospace", "sans-serif"] {
        if let Ok(o) = std::process::Command::new("fc-match")
            .args(["-f", "%{file}", pattern])
            .output()
            && o.status.success()
        {
            let file = String::from_utf8_lossy(&o.stdout).trim().to_string();
            if !file.is_empty() {
                out.push(file);
            }
        }
    }
    out.extend(
        [
            "/usr/share/fonts/TTF/JetBrainsMonoNerdFont-Regular.ttf",
            "/usr/share/fonts/liberation/LiberationSans-Regular.ttf",
            "/usr/share/fonts/TTF/DejaVuSans.ttf",
            "/usr/share/fonts/noto/NotoSans-Regular.ttf",
        ]
        .map(String::from),
    );
    out
}

impl Font {
    /// The first usable font, or None: the chart then draws without text.
    pub fn load() -> Option<Font> {
        candidates().into_iter().find_map(|file| Font::open(&file))
    }

    pub fn open(file: &str) -> Option<Font> {
        let data = std::fs::read(file).ok()?;
        // One font for the life of the process.
        let data: &'static [u8] = Box::leak(data.into_boxed_slice());
        let face = Face::parse(data, 0).ok()?;
        let units = f32::from(face.units_per_em());
        face.glyph_index('0')?;
        Some(Font {
            face,
            units,
            file: file.to_string(),
        })
    }

    fn glyph(&self, c: char) -> Option<GlyphId> {
        self.face
            .glyph_index(c)
            .or_else(|| self.face.glyph_index('?'))
    }

    /// Advance width of a string at a pixel size.
    pub fn width(&self, text: &str, size: f32) -> f32 {
        let k = size / self.units;
        text.chars()
            .filter_map(|c| self.glyph(c))
            .map(|g| f32::from(self.face.glyph_hor_advance(g).unwrap_or(0)) * k)
            .sum()
    }

    /// Cap height as a fraction of the pixel size, for centring digits.
    pub fn cap(&self) -> f32 {
        self.face
            .capital_height()
            .map_or(0.7, |h| f32::from(h) / self.units)
    }

    /// The outline of a string with its baseline starting at (x, y).
    /// Italic slants it, for water names as on a paper chart.
    pub fn path(&self, text: &str, size: f32, x: f32, y: f32, italic: bool) -> Option<Path> {
        let k = size / self.units;
        let mut builder = Builder {
            path: PathBuilder::new(),
            k,
            x,
            y,
            slant: if italic { 0.2 } else { 0.0 },
        };
        for c in text.chars() {
            let Some(g) = self.glyph(c) else { continue };
            self.face.outline_glyph(g, &mut builder);
            builder.x += f32::from(self.face.glyph_hor_advance(g).unwrap_or(0)) * k;
        }
        builder.path.finish()
    }
}

struct Builder {
    path: PathBuilder,
    k: f32,
    x: f32,
    y: f32,
    slant: f32,
}

impl Builder {
    fn at(&self, x: f32, y: f32) -> (f32, f32) {
        (self.x + (x + y * self.slant) * self.k, self.y - y * self.k)
    }
}

impl OutlineBuilder for Builder {
    fn move_to(&mut self, x: f32, y: f32) {
        let (x, y) = self.at(x, y);
        self.path.move_to(x, y);
    }
    fn line_to(&mut self, x: f32, y: f32) {
        let (x, y) = self.at(x, y);
        self.path.line_to(x, y);
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let (x1, y1) = self.at(x1, y1);
        let (x, y) = self.at(x, y);
        self.path.quad_to(x1, y1, x, y);
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let (x1, y1) = self.at(x1, y1);
        let (x2, y2) = self.at(x2, y2);
        let (x, y) = self.at(x, y);
        self.path.cubic_to(x1, y1, x2, y2, x, y);
    }
    fn close(&mut self) {
        self.path.close();
    }
}

/// A transform that rotates about a point, for text along a bearing.
pub fn rotate_about(deg: f32, x: f32, y: f32) -> Transform {
    Transform::from_rotate_at(deg, x, y)
}
