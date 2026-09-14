//! Getting charts: NOAA's free ENCs, a state or a single cell at a time,
//! or a zip or folder already on disk. Downloads go through `curl` and
//! unpack with `bsdtar`, both on every Arch system.

use std::path::{Path, PathBuf};
use std::process::Command;

const NOAA: &str = "https://charts.noaa.gov/ENCs";

/// NOAA's per-state collections, as of 2026. `PO` is the Pacific islands.
pub const REGIONS: &[(&str, &str)] = &[
    ("AK", "Alaska"),
    ("AL", "Alabama"),
    ("CA", "California"),
    ("CT", "Connecticut"),
    ("DE", "Delaware"),
    ("FL", "Florida"),
    ("GA", "Georgia"),
    ("HI", "Hawaii"),
    ("LA", "Louisiana"),
    ("MA", "Massachusetts"),
    ("MD", "Maryland"),
    ("ME", "Maine"),
    ("MI", "Michigan"),
    ("MN", "Minnesota"),
    ("MS", "Mississippi"),
    ("NC", "North Carolina"),
    ("NH", "New Hampshire"),
    ("NJ", "New Jersey"),
    ("NY", "New York"),
    ("OH", "Ohio"),
    ("OR", "Oregon"),
    ("PA", "Pennsylvania"),
    ("PO", "Pacific islands"),
    ("PR", "Puerto Rico and the Virgin Islands"),
    ("RI", "Rhode Island"),
    ("SC", "South Carolina"),
    ("TX", "Texas"),
    ("VA", "Virginia"),
    ("WA", "Washington"),
    ("WI", "Wisconsin"),
];

/// The download for a region code or a cell name.
pub fn url(target: &str) -> Result<String, String> {
    let t = target.trim().to_ascii_uppercase();
    if REGIONS.iter().any(|(code, _)| *code == t) {
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
        REGIONS.iter().map(|r| r.0).collect::<Vec<_>>().join(" ")
    ))
}

fn downloads() -> PathBuf {
    let base = std::env::var("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| crate::style::home().join(".cache"));
    base.join("omahelm/downloads")
}

/// Downloads each target and installs its cells into the charts directory.
pub fn fetch(targets: &[String], root: &Path) -> Result<usize, String> {
    let urls: Vec<String> = targets.iter().map(|t| url(t)).collect::<Result<_, _>>()?;
    let dir = downloads();
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
}
