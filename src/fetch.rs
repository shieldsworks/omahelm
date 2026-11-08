//! Getting charts: NOAA's free ENCs, a state or a single cell at a time,
//! or a zip or folder already on disk. Downloads go through `curl` and
//! unpack with `bsdtar`, both on every Arch system.

use std::path::{Path, PathBuf};
use std::process::Command;

const NOAA: &str = "https://charts.noaa.gov/ENCs";

/// A rectangle in degrees: south, west, north, east. West is less than east.
pub type Bounds = [f64; 4];

/// One NOAA collection and where its charts are.
pub struct Region {
    pub code: &'static str,
    pub name: &'static str,
    /// Generous bounds of the state or territory with the waters its charts
    /// cover, in several rectangles where one would take in open ocean or
    /// cross the antimeridian. Rounded outward from the state's extent plus
    /// its coastal waters and, for the Great Lakes states, the lakes; they
    /// are not NOAA's cell outlines. A collection is picked when an area
    /// touches any of them.
    pub boxes: &'static [Bounds],
}

/// NOAA's per-state collections, as of 2026. `PO` is the Pacific islands:
/// Guam, the Northern Marianas and American Samoa.
pub const REGIONS: &[Region] = &[
    Region {
        code: "AK",
        name: "Alaska",
        boxes: &[[51.0, -180.0, 72.0, -129.0], [51.0, 172.0, 53.5, 180.0]],
    },
    Region {
        code: "AL",
        name: "Alabama",
        boxes: &[[29.8, -88.6, 35.1, -84.8]],
    },
    Region {
        code: "CA",
        name: "California",
        boxes: &[[32.4, -124.8, 42.1, -114.0]],
    },
    Region {
        code: "CT",
        name: "Connecticut",
        boxes: &[[40.8, -73.8, 42.1, -71.7]],
    },
    Region {
        code: "DE",
        name: "Delaware",
        boxes: &[[38.3, -75.9, 39.9, -74.9]],
    },
    Region {
        code: "FL",
        name: "Florida",
        boxes: &[[24.2, -87.7, 31.1, -79.8]],
    },
    Region {
        code: "GA",
        name: "Georgia",
        boxes: &[[30.3, -85.7, 35.1, -80.6]],
    },
    Region {
        code: "HI",
        name: "Hawaii",
        boxes: &[[18.5, -179.0, 29.0, -154.6]],
    },
    Region {
        code: "LA",
        name: "Louisiana",
        boxes: &[[28.8, -94.1, 33.1, -88.7]],
    },
    Region {
        code: "MA",
        name: "Massachusetts",
        boxes: &[[41.1, -73.6, 42.9, -69.6]],
    },
    Region {
        code: "MD",
        name: "Maryland",
        boxes: &[[37.8, -79.6, 39.8, -75.0]],
    },
    Region {
        code: "ME",
        name: "Maine",
        boxes: &[[43.0, -71.2, 47.5, -66.8]],
    },
    Region {
        code: "MI",
        name: "Michigan",
        boxes: &[[41.6, -90.5, 48.4, -82.1]],
    },
    Region {
        code: "MN",
        name: "Minnesota",
        boxes: &[[43.4, -97.3, 49.5, -89.3]],
    },
    Region {
        code: "MS",
        name: "Mississippi",
        boxes: &[[30.0, -91.7, 35.1, -88.0]],
    },
    Region {
        code: "NC",
        name: "North Carolina",
        boxes: &[[33.4, -84.4, 36.7, -75.0]],
    },
    Region {
        code: "NH",
        name: "New Hampshire",
        boxes: &[[42.6, -72.6, 45.4, -70.5]],
    },
    Region {
        code: "NJ",
        name: "New Jersey",
        boxes: &[[38.8, -75.6, 41.4, -73.6]],
    },
    Region {
        code: "NY",
        name: "New York",
        boxes: &[[40.4, -79.8, 45.1, -71.7]],
    },
    Region {
        code: "OH",
        name: "Ohio",
        boxes: &[[38.4, -84.9, 42.1, -80.4]],
    },
    Region {
        code: "OR",
        name: "Oregon",
        boxes: &[[41.9, -124.8, 46.3, -116.4]],
    },
    Region {
        code: "PA",
        name: "Pennsylvania",
        boxes: &[[39.7, -80.6, 42.6, -74.6]],
    },
    Region {
        code: "PO",
        name: "Pacific islands",
        boxes: &[[12.9, 144.5, 20.7, 146.3], [-14.7, -171.2, -14.1, -169.4]],
    },
    Region {
        code: "PR",
        name: "Puerto Rico and the Virgin Islands",
        boxes: &[[17.5, -68.1, 18.8, -64.2]],
    },
    Region {
        code: "RI",
        name: "Rhode Island",
        boxes: &[[41.0, -71.95, 42.05, -71.0]],
    },
    Region {
        code: "SC",
        name: "South Carolina",
        boxes: &[[31.9, -83.4, 35.3, -78.4]],
    },
    Region {
        code: "TX",
        name: "Texas",
        boxes: &[[25.6, -106.7, 36.6, -93.4]],
    },
    Region {
        code: "VA",
        name: "Virginia",
        boxes: &[[36.4, -83.7, 39.5, -74.9]],
    },
    Region {
        code: "WA",
        name: "Washington",
        boxes: &[[45.5, -124.9, 49.1, -116.9]],
    },
    Region {
        code: "WI",
        name: "Wisconsin",
        boxes: &[[42.4, -92.9, 47.1, -86.8]],
    },
];

/// The collections whose bounds touch the area, in table order.
pub fn regions_in(area: Bounds) -> Vec<&'static Region> {
    let [s, w, n, e] = area;
    REGIONS
        .iter()
        .filter(|r| {
            r.boxes
                .iter()
                .any(|b| b[0] <= n && s <= b[2] && b[1] <= e && w <= b[3])
        })
        .collect()
}

/// Reads `S,W,N,E` in degrees: south below north, west below east, so an
/// area never crosses the antimeridian.
pub fn parse_area(text: &str) -> Result<Bounds, String> {
    let bad = || format!("{text} is not an area: expected SOUTH,WEST,NORTH,EAST in degrees");
    let v: Vec<f64> = text
        .split(',')
        .map(|p| p.trim().parse::<f64>().map_err(|_| bad()))
        .collect::<Result<_, _>>()?;
    let [s, w, n, e] = v[..] else {
        return Err(bad());
    };
    if !((-90.0..=90.0).contains(&s) && (-90.0..=90.0).contains(&n)) {
        return Err(format!("{text}: latitudes run from -90 to 90"));
    }
    if !((-180.0..=180.0).contains(&w) && (-180.0..=180.0).contains(&e)) {
        return Err(format!("{text}: longitudes run from -180 to 180"));
    }
    if s > n || w > e {
        return Err(format!(
            "{text}: south must not be above north, nor west east of east"
        ));
    }
    Ok([s, w, n, e])
}

/// The collections for an area, or an error that says what to do when NOAA
/// has none there.
pub fn pick(text: &str) -> Result<Vec<&'static Region>, String> {
    let picked = regions_in(parse_area(text)?);
    if picked.is_empty() {
        return Err(format!(
            "no NOAA ENC collection covers {text}. NOAA charts US waters only; \
             if you have charts for this area, install them with `omahelm import ZIP|DIR`."
        ));
    }
    Ok(picked)
}

/// The download for a region code or a cell name.
pub fn url(target: &str) -> Result<String, String> {
    let t = target.trim().to_ascii_uppercase();
    if REGIONS.iter().any(|r| r.code == t) {
        return Ok(format!("{NOAA}/{t}_ENCs.zip"));
    }
    let cell = t.len() == 8
        && t.starts_with("US")
        && t.as_bytes()[2].is_ascii_digit()
        && t.chars().all(|c| c.is_ascii_alphanumeric());
    if cell {
        return Ok(format!("{NOAA}/{t}.zip"));
    }
    Err(format!(
        "{target} is not a NOAA region or cell. Regions: {}. A cell is named like US5OAKFI.",
        REGIONS.iter().map(|r| r.code).collect::<Vec<_>>().join(" ")
    ))
}

fn downloads() -> Result<PathBuf, String> {
    Ok(crate::paths::Xdg::Cache.base()?.join("omahelm/downloads"))
}

/// Downloads each target and installs its cells into the charts directory.
pub fn fetch(targets: &[String], root: &Path) -> Result<usize, String> {
    let urls: Vec<String> = targets.iter().map(|t| url(t)).collect::<Result<_, _>>()?;
    let dir = downloads()?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let mut installed = 0;
    for u in urls {
        let name = u.rsplit('/').next().unwrap_or("charts.zip");
        let zip = dir.join(name);
        let part = dir.join(format!("{name}.part"));
        eprintln!("Downloading {u}");
        let status = Command::new("curl")
            .args([
                "--fail",
                "--location",
                "--retry",
                "3",
                "--progress-bar",
                "--output",
            ])
            .arg(&part)
            .arg(&u)
            .status()
            .map_err(|e| format!("curl: {e}"))?;
        if !status.success() {
            let _ = std::fs::remove_file(&part);
            return Err(format!("could not download {u}"));
        }
        std::fs::rename(&part, &zip).map_err(|e| format!("{}: {e}", zip.display()))?;
        installed += import(&zip, root)?;
        let _ = std::fs::remove_file(&zip);
    }
    Ok(installed)
}

/// Installs the cells in a zip or a directory (an `ENC_ROOT` or anything
/// holding one), replacing older copies. Returns how many cells.
pub fn import(path: &Path, root: &Path) -> Result<usize, String> {
    let enc_root = root.join("ENC_ROOT");
    std::fs::create_dir_all(&enc_root).map_err(|e| format!("{}: {e}", enc_root.display()))?;
    let staging = root.join(format!(".import-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&staging);
    let source = if path.is_dir() {
        path.to_path_buf()
    } else {
        std::fs::create_dir_all(&staging).map_err(|e| format!("{}: {e}", staging.display()))?;
        let status = Command::new("bsdtar")
            .arg("-xf")
            .arg(path)
            .arg("-C")
            .arg(&staging)
            .status()
            .map_err(|e| format!("bsdtar: {e}"))?;
        if !status.success() {
            let _ = std::fs::remove_dir_all(&staging);
            return Err(format!("could not unpack {}", path.display()));
        }
        staging.clone()
    };
    let result = install_cells(&source, &enc_root, root);
    let _ = std::fs::remove_dir_all(&staging);
    result
}

fn install_cells(source: &Path, enc_root: &Path, root: &Path) -> Result<usize, String> {
    let bases = crate::library::find_cells(source);
    if bases.is_empty() {
        return Err(format!("no ENC cells (*.000) in {}", source.display()));
    }
    let mut count = 0;
    for base in bases {
        let Some(stem) = base.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        let Some(from) = base.parent() else { continue };
        let target = enc_root.join(stem);
        let fresh = root.join(format!(".new-{stem}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&fresh);
        std::fs::create_dir_all(&fresh).map_err(|e| format!("{}: {e}", fresh.display()))?;
        // The cell, its updates and its text notes; nothing else.
        for e in std::fs::read_dir(from)
            .map_err(|e| e.to_string())?
            .flatten()
        {
            let p = e.path();
            let ext = p
                .extension()
                .and_then(|x| x.to_str())
                .unwrap_or("")
                .to_ascii_uppercase();
            let ours = p.file_stem().and_then(|s| s.to_str()) == Some(stem)
                && ext.len() == 3
                && ext.chars().all(|c| c.is_ascii_digit());
            if ours || ext == "TXT" || ext == "TIF" {
                std::fs::copy(&p, fresh.join(e.file_name()))
                    .map_err(|e| format!("{}: {e}", p.display()))?;
            }
        }
        let old = root.join(format!(".old-{stem}-{}", std::process::id()));
        if target.exists() {
            std::fs::rename(&target, &old).map_err(|e| format!("{}: {e}", target.display()))?;
        }
        std::fs::rename(&fresh, &target).map_err(|e| format!("{}: {e}", target.display()))?;
        let _ = std::fs::remove_dir_all(&old);
        count += 1;
    }
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_for_regions_and_cells() {
        assert_eq!(
            url("ca").unwrap(),
            "https://charts.noaa.gov/ENCs/CA_ENCs.zip"
        );
        assert_eq!(
            url("US5OAKFI").unwrap(),
            "https://charts.noaa.gov/ENCs/US5OAKFI.zip"
        );
        assert!(url("ZZ").is_err());
        assert!(url("US5OAK/../x").is_err());
    }

    #[test]
    fn import_replaces_a_cell_and_keeps_its_updates() {
        let tmp = std::env::temp_dir().join(format!("omahelm-import-{}", std::process::id()));
        let src = tmp.join("src/ENC_ROOT/US5TEST1");
        std::fs::create_dir_all(&src).unwrap();
        for f in ["US5TEST1.000", "US5TEST1.001", "US5TEST1A.TXT", "junk.bin"] {
            std::fs::write(src.join(f), f).unwrap();
        }
        let root = tmp.join("charts");
        assert_eq!(import(&tmp.join("src"), &root).unwrap(), 1);
        let cell = root.join("ENC_ROOT/US5TEST1");
        assert!(cell.join("US5TEST1.001").exists());
        assert!(cell.join("US5TEST1A.TXT").exists());
        assert!(!cell.join("junk.bin").exists());
        // Again, as an update would: still one copy.
        assert_eq!(import(&tmp.join("src"), &root).unwrap(), 1);
        assert_eq!(std::fs::read_dir(root.join("ENC_ROOT")).unwrap().count(), 1);
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    fn codes(area: &str) -> Vec<&'static str> {
        pick(area).unwrap().iter().map(|r| r.code).collect()
    }

    #[test]
    fn every_region_has_sane_boxes() {
        for r in REGIONS {
            assert!(!r.boxes.is_empty(), "{}", r.code);
            for [s, w, n, e] in r.boxes {
                assert!(-90.0 <= *s && s < n && *n <= 90.0, "{}", r.code);
                assert!(-180.0 <= *w && w < e && *e <= 180.0, "{}", r.code);
            }
        }
    }

    #[test]
    fn puget_sound_is_washington() {
        assert_eq!(codes("46.7,-123.5,48.7,-121.3"), ["WA"]);
    }

    #[test]
    fn san_francisco_bay_is_california() {
        assert_eq!(codes("37.5,-122.7,38.2,-121.9"), ["CA"]);
    }

    #[test]
    fn a_box_across_a_state_line_takes_both() {
        assert_eq!(codes("41.5,-124.6,42.5,-123.5"), ["CA", "OR"]);
    }

    #[test]
    fn a_point_is_an_area() {
        assert_eq!(codes("21.3,-157.9,21.3,-157.9"), ["HI"]);
    }

    #[test]
    fn the_islands_pick_by_their_own_waters() {
        assert_eq!(codes("13.3,144.6,13.6,144.9"), ["PO"]);
        assert_eq!(codes("17.9,-65.2,18.5,-64.5"), ["PR"]);
        assert_eq!(codes("51.8,175.0,52.0,179.0"), ["AK"]);
        assert_eq!(codes("-14.4,-170.9,-14.2,-170.5"), ["PO"]);
    }

    #[test]
    fn the_mid_atlantic_has_no_collection_and_the_error_says_what_to_do() {
        let err = pick("30,-50,35,-40").err().unwrap();
        assert!(err.contains("no NOAA ENC collection"), "{err}");
        assert!(err.contains("omahelm import"), "{err}");
    }

    #[test]
    fn areas_are_checked() {
        assert_eq!(parse_area(" 1, 2 ,3,4 "), Ok([1.0, 2.0, 3.0, 4.0]));
        for bad in [
            "1,2,3",
            "a,b,c,d",
            "5,0,4,1",
            "0,10,1,5",
            "-91,0,0,1",
            "0,0,1,181",
            "",
        ] {
            assert!(parse_area(bad).is_err(), "{bad}");
        }
    }
}
