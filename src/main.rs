use omahelm::geo::{self, mercator};
use omahelm::library::{self, Library};
use omahelm::render::{self, Style, TILE, TileKey};
use omahelm::s57::{self, Cell, Geometry};
use omahelm::style::{self, Palette, Settings};
use omahelm::text::Font;
use omahelm::trips;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const USAGE: &str = "usage:
  omahelm serve [--charts DIR]            run the chart engine for the chartplotter
  omahelm fetch REGION|CELL... [--charts DIR]
                                          download NOAA charts: a state (CA) or a cell (US5OAKFI)
  omahelm fetch --list                    list NOAA's regions
  omahelm import ZIP|DIR [--charts DIR]   install ENC cells you already have
  omahelm index [--charts DIR]            re-read the charts and list any skipped
  omahelm trips [--logbook DIR]           the days your logbook has tracks for
  omahelm trip [--date YYYY-MM-DD] [--logbook DIR] [--out FILE.gpx]
                                          a day's passages as the one trip they were
  omahelm query --at LAT,LON [--zoom Z] [--charts DIR]
  omahelm render --center LAT,LON --zoom Z [--size WxH] [--scale N] [--night] [--charts DIR] --out FILE.png
  omahelm dump CELL.000 [CLASS]";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("dump") if args.len() >= 2 => {
            dump(Path::new(&args[1]), args.get(2).map(String::as_str))
        }
        Some("index") => index(&args[1..]),
        Some("serve") => omahelm::server::serve(charts_root(&args[1..])),
        Some("fetch") if args.get(1).map(String::as_str) == Some("--list") => {
            for (code, name) in omahelm::fetch::REGIONS {
                println!("{code}  {name}");
            }
            Ok(())
        }
        Some("fetch") if args.len() >= 2 => fetch(&args[1..]),
        Some("import") if args.len() >= 2 => import(&args[1..]),
        Some("trips") => trips(&args[1..]),
        Some("trip") => trip(&args[1..]),
        Some("query") => query(&args[1..]),
        Some("render") => render_view(&args[1..]),
        _ => Err(USAGE.to_string()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("omahelm: {e}");
            ExitCode::FAILURE
        }
    }
}

/// `--name value` from an argument list.
fn flag<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .map(String::as_str)
}

fn charts_root(args: &[String]) -> PathBuf {
    flag(args, "--charts").map_or_else(library::default_root, PathBuf::from)
}

fn open_library(root: &Path) -> Library {
    Library::open(root, &|done, total| {
        if done == total || done % 25 == 0 {
            eprint!("\rreading charts {done}/{total}");
            if done == total {
                eprintln!();
            }
        }
    })
}

/// Arguments that aren't flags or their values.
fn positional(args: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    let mut skip = false;
    for a in args {
        if skip {
            skip = false;
        } else if a.starts_with("--") {
            skip = true;
        } else {
            out.push(a.clone());
        }
    }
    out
}

fn fetch(args: &[String]) -> Result<(), String> {
    let root = charts_root(args);
    let n = omahelm::fetch::fetch(&positional(args), &root)?;
    eprintln!("Installed {n} cells in {}", root.display());
    index(args)
}

fn import(args: &[String]) -> Result<(), String> {
    let root = charts_root(args);
    let mut n = 0;
    for p in positional(args) {
        n += omahelm::fetch::import(Path::new(&p), &root)?;
    }
    eprintln!("Installed {n} cells in {}", root.display());
    index(args)
}

fn query(args: &[String]) -> Result<(), String> {
    let at = flag(args, "--at").ok_or("--at LAT,LON is required")?;
    let (lat, lon) = at
        .split_once(',')
        .and_then(|(a, b)| Some((a.trim().parse::<f64>().ok()?, b.trim().parse::<f64>().ok()?)))
        .ok_or("--at takes LAT,LON")?;
    let zoom = flag(args, "--zoom")
        .and_then(|z| z.parse().ok())
        .unwrap_or(15.0);
    let lib = open_library(&charts_root(args));
    let style = current_style(false);
    for f in omahelm::server::features_at(&lib, &style.settings, lat, lon, zoom) {
        println!("{f}");
    }
    Ok(())
}

fn index(args: &[String]) -> Result<(), String> {
    let root = charts_root(args);
    let lib = open_library(&root);
    println!("{} cells in {}", lib.entries.len(), root.display());
    for (cell, why) in &lib.problems {
        let cell = cell.split('|').next().unwrap_or(cell);
        println!("  skipped {cell}: {why}");
    }
    Ok(())
}

/// The logbook vault: `--logbook`, else the `logbook` setting, else the
/// vault omalogbook is configured for.
fn logbook(args: &[String]) -> PathBuf {
    flag(args, "--logbook").map_or_else(
        || trips::default_vault(current_style(false).settings.logbook.as_deref()),
        trips::expand,
    )
}

fn open_log(args: &[String]) -> Result<trips::Log, String> {
    let log = trips::Log::open(logbook(args));
    match log.status() {
        "none" => Err(format!(
            "no tracks at {}. Set `logbook` in config.toml, or pass --logbook.",
            log.vault().display()
        )),
        "empty" => Err(format!("no GPX tracks in {}", log.vault().display())),
        _ => Ok(log),
    }
}

/// `omahelm trips`: what the logbook has, a day per line.
fn trips(args: &[String]) -> Result<(), String> {
    let log = open_log(args)?;
    let mut total = 0.0;
    for day in log.days() {
        let (from, to) = day.span();
        let nm = day.run_nm() + day.gap_nm();
        total += nm;
        let gaps = day.gaps().count();
        println!(
            "{}  {:>6} nm  {:>2} passage{}  {:>2} gap{} {:>7}  {}",
            day.date,
            trips::format_nm(nm),
            day.passages,
            if day.passages == 1 { " " } else { "s" },
            gaps,
            if gaps == 1 { " " } else { "s" },
            if gaps > 0 {
                format!("({} nm)", trips::format_nm(day.gap_nm()))
            } else {
                String::new()
            },
            if from > 0 {
                format!("{}–{}", trips::local_clock(from), trips::local_clock(to))
            } else {
                String::new()
            }
        );
    }
    eprintln!(
        "{} {}, {} nm, in {}",
        log.days().len(),
        if log.days().len() == 1 { "day" } else { "days" },
        trips::format_nm(total),
        log.vault().display()
    );
    Ok(())
}

/// `omahelm trip`: one day as a single GPX track, its holes closed with
/// straight lines. The newest day unless `--date` says otherwise.
fn trip(args: &[String]) -> Result<(), String> {
    let log = open_log(args)?;
    let day = match flag(args, "--date") {
        Some(d) => log
            .day(d)
            .ok_or_else(|| format!("no tracks for {d} in {}", log.vault().display()))?,
        None => log.days().last().expect("a day, since the log isn't empty"),
    };
    let gpx = day.gpx(&log.boat(&day.date));
    let gaps = day.gaps().count();
    eprintln!(
        "{}: {} passages joined, {} gap{} filled with straight lines, {} of {} nm inferred",
        day.date,
        day.passages,
        gaps,
        if gaps == 1 { "" } else { "s" },
        trips::format_nm(day.gap_nm()),
        trips::format_nm(day.run_nm() + day.gap_nm())
    );
    match flag(args, "--out") {
        Some(out) => {
            std::fs::write(out, &gpx).map_err(|e| format!("{out}: {e}"))?;
            eprintln!("Wrote {out}");
        }
        None => print!("{gpx}"),
    }
    Ok(())
}

fn current_style(night: bool) -> Style {
    let settings = std::fs::read_to_string(style::config_path())
        .map(|t| Settings::parse(&t).0)
        .unwrap_or_default();
    let palette = if night {
        Palette::night()
    } else if settings.palette == "paper" {
        Palette::paper()
    } else {
        Palette::from_theme(&style::read_theme(&style::theme_path()))
    };
    Style { palette, settings }
}

/// Renders a view as one PNG, tile by tile, as the chartplotter would.
fn render_view(args: &[String]) -> Result<(), String> {
    let center = flag(args, "--center").ok_or("--center LAT,LON is required")?;
    let (lat, lon) = center
        .split_once(',')
        .and_then(|(a, b)| Some((a.trim().parse::<f64>().ok()?, b.trim().parse::<f64>().ok()?)))
        .ok_or("--center takes LAT,LON")?;
    let zoom: u32 = flag(args, "--zoom")
        .and_then(|z| z.parse().ok())
        .ok_or("--zoom is required")?;
    let (w, h) = flag(args, "--size")
        .and_then(|s| s.split_once('x'))
        .and_then(|(a, b)| Some((a.parse::<u32>().ok()?, b.parse::<u32>().ok()?)))
        .unwrap_or((1200, 800));
    let scale: u32 = flag(args, "--scale")
        .and_then(|s| s.parse().ok())
        .unwrap_or(2);
    let out = flag(args, "--out").ok_or("--out is required")?;
    let lib = open_library(&charts_root(args));
    let style = current_style(args.iter().any(|a| a == "--night"));
    let font = Font::load();
    let world = f64::from(zoom).exp2() * TILE;
    let [mx, my] = mercator(lon, lat);
    let (left, top) = (
        mx * world - f64::from(w) / 2.0,
        my * world - f64::from(h) / 2.0,
    );
    let s = scale as f32;
    let mut canvas = tiny_skia::Pixmap::new(w * scale, h * scale).ok_or("bad size")?;
    let started = std::time::Instant::now();
    let mut tiles = 0;
    let n = 1u32 << zoom;
    for ty in (top / TILE).floor() as i64..=((top + f64::from(h)) / TILE).floor() as i64 {
        for tx in (left / TILE).floor() as i64..=((left + f64::from(w)) / TILE).floor() as i64 {
            if ty < 0 || tx < 0 || ty >= i64::from(n) || tx >= i64::from(n) {
                continue;
            }
            let key = TileKey {
                z: zoom,
                x: tx as u32,
                y: ty as u32,
                scale,
            };
            let tile = render::render(&lib, &style, font.as_ref(), key)?;
            tiles += 1;
            let (dx, dy) = (
                (tx as f64 * TILE - left) as f32 * s,
                (ty as f64 * TILE - top) as f32 * s,
            );
            canvas.draw_pixmap(
                dx.round() as i32,
                dy.round() as i32,
                tile.as_ref(),
                &tiny_skia::PixmapPaint::default(),
                tiny_skia::Transform::identity(),
                None,
            );
        }
    }
    std::fs::write(out, render::png(&canvas)?).map_err(|e| format!("{out}: {e}"))?;
    eprintln!(
        "{tiles} tiles in {:.0} ms, 1:{:.0}",
        started.elapsed().as_secs_f64() * 1000.0,
        geo::scale_denominator(f64::from(zoom), lat)
    );
    Ok(())
}

/// Prints a cell's features per class, with vertex counts, for checking
/// the reader against another one. With a class, prints each feature.
fn dump(path: &Path, only: Option<&str>) -> Result<(), String> {
    let cell = Cell::open(path)?;
    println!(
        "{} edition {} update {} issued {} scale 1:{}",
        cell.name, cell.edition, cell.update, cell.issued, cell.scale
    );
    let attrs_too = only == Some("--attrs");
    let mut classes: BTreeMap<&str, (usize, usize, usize)> = BTreeMap::new();
    for f in &cell.features {
        let acronym = s57::acronym(f.class);
        let vertices = match &f.geometry {
            Geometry::None => 0,
            Geometry::Point(_) => 1,
            Geometry::Soundings(s) => s.len(),
            Geometry::Lines(l) => l.iter().map(Vec::len).sum(),
            Geometry::Area { rings, .. } => rings.iter().map(Vec::len).sum(),
        };
        let entry = classes.entry(acronym).or_default();
        entry.0 += 1;
        entry.1 += vertices;
        entry.2 += f.attrs.iter().filter(|(_, v)| !v.is_empty()).count();
        if only == Some(acronym) {
            let attrs: Vec<String> = f
                .attrs
                .iter()
                .map(|(c, v)| format!("{}={v}", s57::attribute(*c).map_or("?", |a| a.0)))
                .collect();
            let at = match &f.geometry {
                Geometry::Point(p) => format!("{:.6},{:.6}", p.lat, p.lon),
                Geometry::Soundings(s) => format!("{} soundings", s.len()),
                Geometry::Lines(l) => format!("{} lines", l.len()),
                Geometry::Area { rings, .. } => format!("{} rings", rings.len()),
                Geometry::None => String::new(),
            };
            println!("  {} {} {}", f.id.fidn, at, attrs.join(" "));
        }
    }
    for (acronym, (count, vertices, attrs)) in classes {
        if attrs_too {
            println!("{acronym} {count} {vertices} {attrs}");
        } else {
            println!("{acronym} {count} {vertices}");
        }
    }
    Ok(())
}
