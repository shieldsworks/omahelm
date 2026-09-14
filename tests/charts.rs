//! The reader and renderer against real NOAA cells (tests/fixtures).

use omahelm::library::Library;
use omahelm::render::{self, Style, TileKey};
use omahelm::s57::{self, Cell, Geometry};
use omahelm::style::{Palette, Settings};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

/// Object class → (features, vertices), as `gdaldump.py` counts them.
fn counts(cell: &Cell) -> BTreeMap<String, (usize, usize)> {
    let mut out: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    for f in &cell.features {
        let v = match &f.geometry {
            Geometry::None => 0,
            Geometry::Point(_) => 1,
            Geometry::Soundings(s) => s.len(),
            Geometry::Lines(l) => l.iter().map(Vec::len).sum(),
            Geometry::Area { rings, .. } => rings.iter().map(Vec::len).sum(),
        };
        let e = out.entry(s57::acronym(f.class).to_string()).or_default();
        e.0 += 1;
        e.1 += v;
    }
    out
}

fn gdal(cell: &str) -> BTreeMap<String, (usize, usize)> {
    let text = std::fs::read_to_string(fixtures().join(format!("{cell}.gdal.txt"))).unwrap();
    text.lines()
        .filter_map(|l| {
            let mut w = l.split_whitespace();
            Some((
                w.next()?.to_string(),
                (w.next()?.parse().ok()?, w.next()?.parse().ok()?),
            ))
        })
        .collect()
}

fn open(cell: &str) -> Cell {
    Cell::open(&fixtures().join(format!("ENC_ROOT/{cell}/{cell}.000"))).unwrap()
}

#[test]
fn emeryville_matches_gdal_class_by_class() {
    let cell = open("US5OAKFI");
    assert_eq!(cell.name, "US5OAKFI");
    assert_eq!((cell.edition, cell.update, cell.scale), (1, 0, 12000));
    assert_eq!(counts(&cell), gdal("US5OAKFI"));
}

#[test]
fn alcatraz_with_two_updates_matches_gdal() {
    let cell = open("US5OAKFG");
    assert_eq!(cell.update, 2);
    assert_eq!(cell.issued, "20260422");
    assert_eq!(counts(&cell), gdal("US5OAKFG"));
}

#[test]
fn berkeley_breakwater_light_reads_as_charted() {
    let cell = open("US5OAKFI");
    let objnam = 116;
    let beacon = cell
        .features
        .iter()
        .find(|f| f.attr(objnam) == Some("Berkeley North Breakwater Light 4"))
        .expect("the breakwater light");
    assert_eq!(s57::acronym(beacon.class), "BCNLAT");
    // Its slaves are its daymark and its light.
    let light = cell
        .features
        .iter()
        .find(|f| beacon.slaves.contains(&f.id) && s57::acronym(f.class) == "LIGHTS")
        .expect("its light");
    let get = |code| light.attr(code).unwrap();
    // LITCHR 2 flashing, COLOUR 3 red, SIGPER 4 s, VALNMR 4 nm.
    assert_eq!(
        (get(107), get(75), get(142), get(178)),
        ("2", "3", "4", "4")
    );
    let Geometry::Point(p) = light.geometry else {
        panic!("a point")
    };
    assert!((p.lat - 37.868315).abs() < 1e-6 && (p.lon + 122.320472).abs() < 1e-6);
}

#[test]
fn a_harbor_tile_draws_chart_not_blank() {
    let tmp = std::env::temp_dir().join(format!("omahelm-tile-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(tmp.join("ENC_ROOT")).unwrap();
    for cell in ["US5OAKFI", "US5OAKFG"] {
        let dst = tmp.join("ENC_ROOT").join(cell);
        std::fs::create_dir_all(&dst).unwrap();
        for e in std::fs::read_dir(fixtures().join("ENC_ROOT").join(cell))
            .unwrap()
            .flatten()
        {
            std::fs::copy(e.path(), dst.join(e.file_name())).unwrap();
        }
    }
    let lib = Library::open(&tmp, &|_, _| {});
    assert_eq!(lib.entries.len(), 2);
    let style = Style {
        palette: Palette::paper(),
        settings: Settings::default(),
    };
    // The zoom 15 tile holding the Berkeley breakwater light.
    let [mx, my] = omahelm::geo::mercator(-122.320472, 37.868315);
    let key = TileKey {
        z: 15,
        x: (mx * 32768.0) as u32,
        y: (my * 32768.0) as u32,
        scale: 1,
    };
    let pm = render::render(&lib, &style, None, key).unwrap();
    let mut colors = std::collections::HashSet::new();
    for px in pm.data().as_chunks::<4>().0 {
        colors.insert([px[0], px[1], px[2]]);
    }
    // Land, several depth shades, contours and buoys.
    assert!(colors.len() > 20, "only {} colors", colors.len());
    let land = Palette::paper().land;
    assert!(colors.contains(&[land.0, land.1, land.2]));
    let png = render::png(&pm).unwrap();
    assert_eq!(&png[1..4], b"PNG");
    std::fs::remove_dir_all(&tmp).unwrap();
}
