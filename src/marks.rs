//! Chart notation: how a light, a buoy, a bottom or a color is written on
//! a paper chart. Shared by the tiles and by what a click on a feature
//! shows.

use crate::chart::Item;
use crate::s57::names::*;
use crate::style::Units;

/// A light's character in chart notation, such as `Fl(2) G 6s 5M`.
pub fn light(item: &Item) -> String {
    let mut out = String::new();
    let chr = match item.first(LITCHR) {
        Some(1) => "F",
        Some(2) => "Fl",
        Some(3) => "LFl",
        Some(4) => "Q",
        Some(5) => "VQ",
        Some(6) => "UQ",
        Some(7) => "Iso",
        Some(8) => "Oc",
        Some(9) => "IQ",
        Some(10) => "IVQ",
        Some(11) => "IUQ",
        Some(12) => "Mo",
        Some(13) => "FFl",
        Some(14) => "Fl+LFl",
        Some(15) => "OcFl",
        Some(16) => "FLFl",
        Some(17) => "Al.Oc",
        Some(18) => "Al.LFl",
        Some(19) => "Al.Fl",
        Some(20) => "Al.Gr",
        Some(25) => "Q+LFl",
        Some(26) => "VQ+LFl",
        Some(27) => "UQ+LFl",
        Some(28) => "Al",
        Some(29) => "Al.FFl",
        _ => "",
    };
    out.push_str(chr);
    if let Some(g) = item.attr(SIGGRP) {
        let g = g.trim();
        if !g.is_empty() && g != "()" && g != "(1)" {
            out.push_str(g);
        }
    }
    // White goes unsaid on a light of one color, not on one that
    // alternates white and red.
    let list = item.list(COLOUR);
    let colors: String = list
        .iter()
        .filter_map(|c| match c {
            1 if list.len() > 1 => Some("W"),
            3 => Some("R"),
            4 => Some("G"),
            6 => Some("Y"),
            5 => Some("Bu"),
            9 => Some("Am"),
            11 => Some("Or"),
            _ => None,
        })
        .collect();
    if !colors.is_empty() {
        push_word(&mut out, &colors);
    }
    if let Some(p) = item.num(SIGPER) {
        push_word(&mut out, &format!("{}s", trim_number(p)));
    }
    if let Some(r) = item.num(VALNMR) {
        push_word(&mut out, &format!("{}M", trim_number(r)));
    }
    out
}

fn push_word(out: &mut String, word: &str) {
    if !out.is_empty() {
        out.push(' ');
    }
    out.push_str(word);
}

/// A number without a trailing `.0`.
pub fn trim_number(v: f64) -> String {
    if (v - v.round()).abs() < 1e-9 {
        format!("{}", v.round() as i64)
    } else {
        let s = format!("{v:.1}");
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

pub fn color_letter(code: u32) -> &'static str {
    match code {
        1 => "W",
        2 => "B",
        3 => "R",
        4 => "G",
        5 => "Bu",
        6 => "Y",
        7 => "Gy",
        8 => "Br",
        9 => "Am",
        10 => "Vi",
        11 => "Or",
        12 => "Mg",
        13 => "Pk",
        _ => "",
    }
}

pub fn color_name(code: u32) -> &'static str {
    match code {
        1 => "white",
        2 => "black",
        3 => "red",
        4 => "green",
        5 => "blue",
        6 => "yellow",
        7 => "gray",
        8 => "brown",
        9 => "amber",
        10 => "violet",
        11 => "orange",
        12 => "magenta",
        13 => "pink",
        _ => "unknown color",
    }
}

/// A buoy or beacon's label as a paper chart writes it: color, then the
/// number from its name in quotes, `G "1"` or `R "2"`.
pub fn aid_label(item: &Item) -> String {
    let colors: String = item.list(COLOUR).iter().map(|&c| color_letter(c)).collect();
    // The last word when it's a number (`7`, `2A`) or a short letter code
    // (`BR`), not the end of a name (`Buoy`).
    let number = item
        .attr(OBJNAM)
        .and_then(|n| n.split_whitespace().last())
        .filter(|w| {
            w.len() <= 4
                && w.chars().all(|c| c.is_ascii_alphanumeric())
                && (w.chars().any(|c| c.is_ascii_digit())
                    || w.chars().all(|c| c.is_ascii_uppercase()))
        });
    match number {
        Some(n) if colors.is_empty() => format!("\"{n}\""),
        Some(n) => format!("{colors} \"{n}\""),
        None => colors,
    }
}

/// The bottom's nature, as the abbreviations on a paper chart: `S`, `M`,
/// `Sh`, `Rk`.
pub fn bottom(item: &Item) -> String {
    let quality: Vec<&str> = item
        .list(NATQUA)
        .iter()
        .map(|q| match q {
            1 => "f",
            2 => "m",
            3 => "c",
            4 => "bk",
            5 => "sy",
            6 => "so",
            7 => "sf",
            8 => "v",
            9 => "ca",
            10 => "h",
            _ => "",
        })
        .collect();
    item.list(NATSUR)
        .iter()
        .enumerate()
        .map(|(i, n)| {
            let s = match n {
                1 => "M",
                2 => "Cy",
                3 => "Si",
                4 => "S",
                5 => "St",
                6 => "G",
                7 => "P",
                8 => "Cb",
                9 => "Rk",
                11 => "Lv",
                14 => "Co",
                17 => "Sh",
                18 => "Bo",
                _ => "",
            };
            format!("{}{s}", quality.get(i).copied().unwrap_or(""))
        })
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(".")
}

/// A depth in the chosen units: whole feet or fathoms; meters keep a
/// tenth below 31 m, which is drawn as a subscript.
pub fn depth(meters: f64, units: Units) -> (String, Option<String>) {
    let v = units.from_meters(meters);
    let neg = v < 0.0;
    let a = v.abs();
    let (whole, tenth) = match units {
        Units::Meters if a < 31.0 => {
            let t = (a * 10.0).round() as i64;
            (t / 10, Some(t % 10))
        }
        _ => (a.round() as i64, None),
    };
    let main = if neg {
        format!("-{whole}")
    } else {
        whole.to_string()
    };
    (main, tenth.filter(|t| *t != 0).map(|t| t.to_string()))
}

/// A depth written out, for the identify panel. Feet are whole, as on a
/// US chart, which also undoes NOAA's rounding: 6 ft is stored as 1.8 m.
pub fn depth_words(meters: f64, units: Units) -> String {
    format!("{} {}", depth_number(meters, units), units.short())
}

pub fn depth_number(meters: f64, units: Units) -> String {
    let v = units.from_meters(meters);
    match units {
        Units::Feet => trim_number(v.round()),
        _ => trim_number((v * 10.0).round() / 10.0),
    }
}

pub fn buoy_shape(code: u32) -> &'static str {
    match code {
        1 => "conical (nun)",
        2 => "can",
        3 => "spherical",
        4 => "pillar",
        5 => "spar",
        6 => "barrel",
        7 => "super-buoy",
        8 => "ice buoy",
        _ => "",
    }
}

pub fn water_level(code: u32) -> &'static str {
    match code {
        1 => "partly submerged at high water",
        2 => "always dry",
        3 => "always under water",
        4 => "covers and uncovers",
        5 => "awash",
        6 => "subject to inundation",
        7 => "floating",
        _ => "",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chart::Geom;
    use crate::geo::Rect;

    fn item(attrs: &[(u16, &str)]) -> Item {
        Item {
            class: LIGHTS,
            id: Default::default(),
            attrs: attrs.iter().map(|(c, v)| (*c, v.to_string())).collect(),
            geom: Geom::None,
            bbox: Rect::EMPTY,
            scamin: f64::INFINITY,
            slaves: vec![],
            master: None,
        }
    }

    #[test]
    fn light_notation() {
        let l = item(&[
            (LITCHR, "2"),
            (SIGGRP, "(1)"),
            (COLOUR, "4"),
            (SIGPER, "4"),
            (VALNMR, "4"),
        ]);
        assert_eq!(light(&l), "Fl G 4s 4M");
        let l = item(&[
            (LITCHR, "2"),
            (SIGGRP, "(2)"),
            (COLOUR, "1"),
            (SIGPER, "2.5"),
        ]);
        assert_eq!(light(&l), "Fl(2) 2.5s");
        let l = item(&[(LITCHR, "4"), (COLOUR, "3")]);
        assert_eq!(light(&l), "Q R");
        let l = item(&[(LITCHR, "28"), (COLOUR, "1,3"), (SIGPER, "10")]);
        assert_eq!(light(&l), "Al WR 10s");
    }

    #[test]
    fn aid_labels() {
        let b = item(&[(COLOUR, "4"), (OBJNAM, "Emeryville Marina Light 7")]);
        assert_eq!(aid_label(&b), "G \"7\"");
        let b = item(&[(COLOUR, "3,4,3"), (OBJNAM, "Blossom Rock Junction Buoy BR")]);
        assert_eq!(aid_label(&b), "RGR \"BR\"");
        let b = item(&[(COLOUR, "6"), (OBJNAM, "Anchorage Buoy")]);
        assert_eq!(aid_label(&b), "Y");
    }

    #[test]
    fn depths_in_units() {
        assert_eq!(depth(3.6576, Units::Feet), ("12".into(), None));
        assert_eq!(depth(3.4, Units::Meters), ("3".into(), Some("4".into())));
        assert_eq!(depth(35.0, Units::Meters), ("35".into(), None));
        assert_eq!(depth(-0.6, Units::Feet), ("-2".into(), None));
        assert_eq!(depth_words(1.8, Units::Feet), "6 ft");
        assert_eq!(depth_words(1.8, Units::Meters), "1.8 m");
    }
}
