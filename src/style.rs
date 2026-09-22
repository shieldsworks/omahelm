//! How the chart looks: the palette, derived from the Omarchy theme, and
//! the depth settings that decide what counts as shallow.
//!
//! The theme owns the ground: deep water is its background, text and lines
//! its foreground. Colors that carry meaning at sea keep their meaning:
//! a red buoy is red and a green one green whatever the theme calls red
//! and green, so those come from the theme only when its hue is right.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    pub fn parse(s: &str) -> Option<Rgb> {
        let h = s.trim().trim_matches('"').strip_prefix('#')?;
        if h.len() != 6 {
            return None;
        }
        let v = u32::from_str_radix(h, 16).ok()?;
        Some(Rgb((v >> 16) as u8, (v >> 8) as u8, v as u8))
    }

    pub fn mix(self, other: Rgb, t: f32) -> Rgb {
        let f = |a: u8, b: u8| (f32::from(a) + (f32::from(b) - f32::from(a)) * t).round() as u8;
        Rgb(f(self.0, other.0), f(self.1, other.1), f(self.2, other.2))
    }

    /// Relative luminance, 0 to 1.
    pub fn luminance(self) -> f32 {
        let lin = |c: u8| {
            let c = f32::from(c) / 255.0;
            if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * lin(self.0) + 0.7152 * lin(self.1) + 0.0722 * lin(self.2)
    }

    /// Hue in degrees and saturation, 0 to 1.
    pub fn hue(self) -> (f32, f32) {
        let (r, g, b) = (
            f32::from(self.0) / 255.0,
            f32::from(self.1) / 255.0,
            f32::from(self.2) / 255.0,
        );
        let max = r.max(g).max(b);
        let min = r.min(g).min(b);
        let d = max - min;
        if d < 1e-6 {
            return (0.0, 0.0);
        }
        let h = if max == r {
            60.0 * (((g - b) / d) % 6.0)
        } else if max == g {
            60.0 * ((b - r) / d + 2.0)
        } else {
            60.0 * ((r - g) / d + 4.0)
        };
        (h.rem_euclid(360.0), d / max)
    }

    pub fn hex(self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.0, self.1, self.2)
    }
}

/// Every color the chart draws with.
#[derive(Clone, Debug, PartialEq)]
pub struct Palette {
    pub dark: bool,
    /// Outside any chart.
    pub nodata: Rgb,
    pub deep: Rgb,
    pub medium_deep: Rgb,
    pub medium_shallow: Rgb,
    pub very_shallow: Rgb,
    /// Dries at low water.
    pub drying: Rgb,
    pub land: Rgb,
    pub built: Rgb,
    /// Piers, breakwaters, buildings.
    pub structure: Rgb,
    pub ink: Rgb,
    pub faint: Rgb,
    pub contour: Rgb,
    pub magenta: Rgb,
    pub red: Rgb,
    pub green: Rgb,
    pub yellow: Rgb,
    pub white: Rgb,
    pub black: Rgb,
    pub orange: Rgb,
    pub blue: Rgb,
}

/// The standard color for a hue family, and the hue range a theme color
/// must fall in to stand for it.
fn semantic(theme: Option<Rgb>, lo: f32, hi: f32, fallback: Rgb) -> Rgb {
    match theme {
        Some(c) => {
            let (h, s) = c.hue();
            let ok = if lo <= hi {
                h >= lo && h <= hi
            } else {
                h >= lo || h <= hi
            };
            if ok && s > 0.35 { c } else { fallback }
        }
        None => fallback,
    }
}

impl Palette {
    /// The palette for a theme's colors (Omarchy's `colors.toml` keys).
    pub fn from_theme(colors: &HashMap<String, Rgb>) -> Palette {
        let bg = colors
            .get("background")
            .copied()
            .unwrap_or(Rgb(0x1a, 0x1b, 0x26));
        let fg = colors
            .get("foreground")
            .copied()
            .unwrap_or(Rgb(0xa9, 0xb1, 0xd6));
        let dark = bg.luminance() < fg.luminance();
        let get = |k: &str| colors.get(k).copied();
        let water = semantic(get("blue"), 180.0, 250.0, Rgb(0x3d, 0x8b, 0xd8));
        let red = semantic(get("red"), 340.0, 15.0, Rgb(0xd8, 0x3a, 0x3a));
        let green = semantic(get("green"), 85.0, 165.0, Rgb(0x2e, 0xa8, 0x4f));
        let yellow = semantic(get("yellow"), 40.0, 65.0, Rgb(0xe8, 0xc5, 0x2a));
        let magenta = semantic(get("magenta"), 280.0, 335.0, Rgb(0xc0, 0x4c, 0xc8));
        let orange = semantic(get("orange"), 18.0, 40.0, Rgb(0xe0, 0x8a, 0x2a));
        // The land tint is buff, as on a paper chart, faint on the ground.
        let buff = Rgb(0xc8, 0xa8, 0x6a);
        let (land_t, shallow) = if dark {
            (0.22, [0.10, 0.20, 0.34])
        } else {
            (0.45, [0.12, 0.24, 0.40])
        };
        Palette {
            dark,
            nodata: bg.mix(fg, 0.06),
            deep: bg,
            medium_deep: bg.mix(water, shallow[0]),
            medium_shallow: bg.mix(water, shallow[1]),
            very_shallow: bg.mix(water, shallow[2]),
            drying: bg.mix(green, if dark { 0.22 } else { 0.35 }),
            land: bg.mix(buff, land_t),
            // Built-up land barely differs: coarse and fine charts disagree on
            // where towns end, and a stark tint shows their seams.
            built: bg.mix(buff, land_t + 0.03),
            structure: bg.mix(fg, if dark { 0.38 } else { 0.5 }),
            ink: fg,
            faint: bg.mix(fg, 0.55),
            contour: bg.mix(water, if dark { 0.55 } else { 0.6 }).mix(fg, 0.15),
            magenta: if dark {
                magenta.mix(Rgb(255, 255, 255), 0.12)
            } else {
                magenta
            },
            red,
            green,
            yellow,
            white: if dark {
                fg.mix(Rgb(255, 255, 255), 0.4)
            } else {
                Rgb(0xfa, 0xfa, 0xfa)
            },
            black: if dark {
                bg.mix(Rgb(0, 0, 0), 0.5)
            } else {
                Rgb(0x20, 0x20, 0x20)
            },
            orange,
            blue: water,
        }
    }

    /// The colors of a paper chart, for anyone who wants them.
    pub fn paper() -> Palette {
        let mut colors = HashMap::new();
        colors.insert("background".to_string(), Rgb(0xff, 0xff, 0xff));
        colors.insert("foreground".to_string(), Rgb(0x1e, 0x1e, 0x1e));
        let mut p = Palette::from_theme(&colors);
        p.land = Rgb(0xef, 0xdc, 0xa8);
        p.built = Rgb(0xea, 0xd6, 0xa0);
        p.medium_deep = Rgb(0xe4, 0xef, 0xf8);
        p.medium_shallow = Rgb(0xc4, 0xde, 0xf3);
        p.very_shallow = Rgb(0x9c, 0xc8, 0xec);
        p.drying = Rgb(0xb5, 0xd3, 0xa0);
        p
    }

    /// Night Watch: red on black whatever the theme, to keep night vision.
    /// Every color is a red, so marks are told apart by brightness, shape
    /// and label: a red buoy is bright, a green one dark, a yellow or white
    /// one pale, and green cans stay square, red nuns pointed.
    pub fn night() -> Palette {
        let bg = Rgb(0x0c, 0x04, 0x04);
        let hex = |s: &str| Rgb::parse(s).expect("palette color");
        Palette {
            dark: true,
            nodata: hex("#120606"),
            deep: bg,
            // Water stays dark, land stands clear of it.
            medium_deep: hex("#110504"),
            medium_shallow: hex("#170605"),
            very_shallow: hex("#1f0807"),
            drying: hex("#2c0c09"),
            land: hex("#4a170f"),
            built: hex("#4e1910"),
            structure: hex("#6e1e16"),
            ink: hex("#c8402f"),
            faint: hex("#7a2418"),
            contour: hex("#5a1a12"),
            magenta: hex("#a8302a"),
            red: hex("#ff3b2f"),
            // Dark, but clear of the water and the contours.
            green: hex("#7c2117"),
            yellow: hex("#ffa28a"),
            white: hex("#ffb8a4"),
            black: hex("#050101"),
            orange: hex("#ff6b5a"),
            blue: hex("#3a100c"),
        }
    }

    /// A short fingerprint, so cached tiles change when the palette does.
    pub fn key(&self) -> String {
        format!("{self:?}")
    }
}

/// Reads an Omarchy `colors.toml`: `key = "#rrggbb"` lines.
pub fn read_theme(path: &Path) -> HashMap<String, Rgb> {
    let mut out = HashMap::new();
    let Ok(text) = std::fs::read_to_string(path) else {
        return out;
    };
    for line in text.lines() {
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        if let Some(c) = Rgb::parse(v) {
            out.insert(k.trim().to_string(), c);
        }
    }
    out
}

pub fn theme_path() -> PathBuf {
    if let Ok(dir) = std::env::var("OMAHELM_THEME_DIR") {
        return PathBuf::from(dir).join("colors.toml");
    }
    home().join(".local/state/omarchy/current/theme/colors.toml")
}

pub fn home() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/".into()))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Units {
    Feet,
    Meters,
    Fathoms,
}

impl Units {
    pub fn from_meters(self, m: f64) -> f64 {
        match self {
            Units::Feet => m / 0.3048,
            Units::Meters => m,
            Units::Fathoms => m / 1.8288,
        }
    }

    pub fn to_meters(self, v: f64) -> f64 {
        match self {
            Units::Feet => v * 0.3048,
            Units::Meters => v,
            Units::Fathoms => v * 1.8288,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Units::Feet => "feet",
            Units::Meters => "meters",
            Units::Fathoms => "fathoms",
        }
    }

    pub fn short(self) -> &'static str {
        match self {
            Units::Feet => "ft",
            Units::Meters => "m",
            Units::Fathoms => "fm",
        }
    }
}

/// Depth settings, held in meters. The config file gives them in `units`.
#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub units: Units,
    /// Soundings this shallow or less are drawn bold.
    pub safety_depth: f64,
    pub shallow_contour: f64,
    /// Water shallower than this is shaded as unsafe.
    pub safety_contour: f64,
    pub deep_contour: f64,
    /// `theme` or `paper`.
    pub palette: String,
    /// The logbook vault the trips layer reads, when it isn't the one
    /// omalogbook is configured for.
    pub logbook: Option<String>,
}

impl Default for Settings {
    /// For a small keelboat: 6, 12 and 30 foot contours, which NOAA's
    /// charts carry, and 10 feet as the safety depth.
    fn default() -> Self {
        Settings {
            units: Units::Feet,
            safety_depth: 10.0 * 0.3048,
            shallow_contour: 6.0 * 0.3048,
            safety_contour: 12.0 * 0.3048,
            deep_contour: 30.0 * 0.3048,
            palette: "theme".into(),
            logbook: None,
        }
    }
}

impl Settings {
    /// Reads `config.toml`. Unknown keys and bad values are reported and
    /// left at their defaults.
    pub fn parse(text: &str) -> (Settings, Vec<String>) {
        let mut s = Settings::default();
        let mut problems = Vec::new();
        let mut values: Vec<(String, String)> = Vec::new();
        for (n, line) in text.lines().enumerate() {
            let line = line.split('#').next().unwrap_or("").trim();
            if line.is_empty() || line.starts_with('[') {
                continue;
            }
            match line.split_once('=') {
                Some((k, v)) => {
                    values.push((k.trim().to_string(), v.trim().trim_matches('"').to_string()))
                }
                None => problems.push(format!("line {}: expected key = value", n + 1)),
            }
        }
        // Units first, so depths given before them still read in them.
        if let Some((_, v)) = values.iter().find(|(k, _)| k == "units") {
            match v.as_str() {
                "feet" | "ft" => s.units = Units::Feet,
                "meters" | "metres" | "m" => s.units = Units::Meters,
                "fathoms" | "fm" => s.units = Units::Fathoms,
                other => problems.push(format!("units: {other} is not feet, meters or fathoms")),
            }
        }
        for (k, v) in &values {
            let depth = |problems: &mut Vec<String>| match v.parse::<f64>() {
                Ok(d) if (0.0..12_000.0).contains(&d) => Some(s.units.to_meters(d)),
                _ => {
                    problems.push(format!("{k}: {v} is not a depth"));
                    None
                }
            };
            match k.as_str() {
                "units" => {}
                "safety_depth" => s.safety_depth = depth(&mut problems).unwrap_or(s.safety_depth),
                "shallow_contour" => {
                    s.shallow_contour = depth(&mut problems).unwrap_or(s.shallow_contour)
                }
                "safety_contour" => {
                    s.safety_contour = depth(&mut problems).unwrap_or(s.safety_contour)
                }
                "deep_contour" => s.deep_contour = depth(&mut problems).unwrap_or(s.deep_contour),
                "logbook" => {
                    if v.is_empty() {
                        problems.push("logbook: expected a folder".into());
                    } else {
                        s.logbook = Some(v.clone());
                    }
                }
                "palette" => match v.as_str() {
                    "theme" | "paper" => s.palette = v.clone(),
                    other => problems.push(format!("palette: {other} is not theme or paper")),
                },
                other => problems.push(format!("unknown setting {other}")),
            }
        }
        if !(s.shallow_contour <= s.safety_contour && s.safety_contour <= s.deep_contour) {
            problems.push("contours must run shallow ≤ safety ≤ deep; using the defaults".into());
            let d = Settings::default();
            s.shallow_contour = d.shallow_contour;
            s.safety_contour = d.safety_contour;
            s.deep_contour = d.deep_contour;
        }
        (s, problems)
    }

    /// What the tiles are drawn from. The logbook is left out: it changes
    /// nothing about the chart, and pointing it somewhere else mustn't
    /// throw away every tile in the cache.
    pub fn key(&self) -> String {
        format!(
            "{:?}",
            (
                self.units,
                self.safety_depth,
                self.shallow_contour,
                self.safety_contour,
                self.deep_contour,
                &self.palette,
            )
        )
    }
}

pub fn config_path() -> PathBuf {
    if let Ok(p) = std::env::var("OMAHELM_CONFIG") {
        return PathBuf::from(p);
    }
    let base = std::env::var("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| home().join(".config"));
    base.join("omahelm/config.toml")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn theme_red_and_green_keep_their_meaning() {
        // Omarchy's Matte Black calls amber "green" and orange "blue".
        let mut c = HashMap::new();
        c.insert("background".into(), Rgb::parse("#121212").unwrap());
        c.insert("foreground".into(), Rgb::parse("#bebebe").unwrap());
        c.insert("green".into(), Rgb::parse("#FFC107").unwrap());
        c.insert("red".into(), Rgb::parse("#D35F5F").unwrap());
        c.insert("blue".into(), Rgb::parse("#e68e0d").unwrap());
        let p = Palette::from_theme(&c);
        assert!(p.dark);
        assert_eq!(p.red, Rgb::parse("#D35F5F").unwrap());
        let (green_hue, _) = p.green.hue();
        assert!((85.0..165.0).contains(&green_hue));
        let (water_hue, _) = p.blue.hue();
        assert!((180.0..250.0).contains(&water_hue));
        assert_eq!(p.deep, Rgb::parse("#121212").unwrap());
    }

    #[test]
    fn night_is_all_red() {
        let p = Palette::night();
        let all = [
            p.nodata,
            p.deep,
            p.medium_deep,
            p.medium_shallow,
            p.very_shallow,
            p.drying,
            p.land,
            p.built,
            p.structure,
            p.ink,
            p.faint,
            p.contour,
            p.magenta,
            p.red,
            p.green,
            p.yellow,
            p.white,
            p.black,
            p.orange,
            p.blue,
        ];
        for c in all {
            let (h, _) = c.hue();
            assert!(!(20.0..340.0).contains(&h), "{} isn't a red", c.hex());
        }
        // Red and green marks must still differ at a glance.
        assert!(p.red.luminance() > 4.0 * p.green.luminance());
        // Shallower water is lighter, as by day.
        assert!(p.deep.luminance() < p.medium_deep.luminance());
        assert!(p.medium_deep.luminance() < p.medium_shallow.luminance());
        assert!(p.medium_shallow.luminance() < p.very_shallow.luminance());
        // Land is never mistaken for shallow water.
        assert!(p.land.luminance() > 2.0 * p.very_shallow.luminance());
    }

    #[test]
    fn settings_read_in_their_units() {
        let (s, problems) = Settings::parse(
            "safety_depth = 2\nunits = \"metres\"\nsafety_contour = 5 # boat\nbogus = 1\n",
        );
        assert_eq!(s.units, Units::Meters);
        assert_eq!(s.safety_depth, 2.0);
        assert_eq!(s.safety_contour, 5.0);
        assert_eq!(problems, vec!["unknown setting bogus".to_string()]);
        let (s, problems) = Settings::parse("logbook = \"~/Sailing/Log\"\n");
        assert_eq!(s.logbook.as_deref(), Some("~/Sailing/Log"));
        assert!(problems.is_empty());
        // The logbook is not part of how a tile is drawn.
        let mut other = s.clone();
        other.logbook = Some("/elsewhere".into());
        assert_eq!(s.key(), other.key());
        let (s, _) = Settings::parse("");
        assert!((s.safety_contour - 3.6576).abs() < 1e-9);
    }
}
