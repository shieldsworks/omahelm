mod common;

use common::{Charts, fixtures};
use omahelm::render::{self, Style, TileKey};
use omahelm::style::{Palette, Settings};
use omahelm::text::Font;
use std::path::PathBuf;

#[derive(Clone, Copy)]
enum Look {
    Paper,
    Night,
}

struct Golden {
    name: &'static str,
    key: TileKey,
    look: Look,
}

const fn tile(z: u32, x: u32, y: u32) -> TileKey {
    TileKey { z, x, y, scale: 2 }
}

const GOLDENS: &[Golden] = &[
    Golden {
        name: "berkeley-breakwater-paper",
        key: tile(15, 5250, 12654),
        look: Look::Paper,
    },
    Golden {
        name: "berkeley-breakwater-night",
        key: tile(15, 5250, 12654),
        look: Look::Night,
    },
    Golden {
        name: "alcatraz-paper",
        key: tile(14, 2620, 6329),
        look: Look::Paper,
    },
];

const VERSION_FILE: &str = "render-version.txt";

fn golden_dir() -> PathBuf {
    fixtures().with_file_name("golden")
}

fn draw(charts: &Charts, font: &Font, g: &Golden) -> Vec<u8> {
    let palette = match g.look {
        Look::Paper => Palette::paper(),
        Look::Night => Palette::night(),
    };
    let style = Style {
        palette,
        settings: Settings::default(),
    };
    let pm = render::render(&charts.lib, &style, Some(font), g.key).expect("a tile");
    render::png(&pm).expect("a PNG")
}

fn rgb(png: &[u8]) -> Option<Vec<u8>> {
    let mut r = png::Decoder::new(std::io::Cursor::new(png))
        .read_info()
        .ok()?;
    let mut buf = vec![0; r.output_buffer_size()?];
    r.next_frame(&mut buf).ok()?;
    Some(buf)
}

fn difference(golden: &[u8], drawn: &[u8]) -> String {
    match (rgb(golden), rgb(drawn)) {
        (Some(a), Some(b)) if a.len() == b.len() => {
            let (mut n, mut most) = (0, 0);
            for (p, q) in a.chunks(3).zip(b.chunks(3)) {
                let d = p
                    .iter()
                    .zip(q)
                    .map(|(x, y)| x.abs_diff(*y))
                    .max()
                    .unwrap_or(0);
                if d > 0 {
                    n += 1;
                    most = most.max(d);
                }
            }
            format!("{n} of {} pixels differ, by up to {most}", a.len() / 3)
        }
        _ => "not the same size".into(),
    }
}

enum State {
    Same,
    Changed(String),
    New,
}

struct Survey {
    recorded: Option<u32>,
    cases: Vec<(&'static Golden, Vec<u8>, State)>,
    orphans: Vec<PathBuf>,
}

fn survey() -> Survey {
    let charts = Charts::open();
    let font_file = fixtures().join("fonts/DejaVuSansMono.ttf");
    let font =
        Font::open(font_file.to_str().expect("the font path is utf-8")).expect("the pinned font");
    let dir = golden_dir();
    let cases = GOLDENS
        .iter()
        .map(|g| {
            let drawn = draw(&charts, &font, g);
            let state = match std::fs::read(dir.join(format!("{}.png", g.name))) {
                Ok(golden) if golden == drawn => State::Same,
                Ok(golden) => State::Changed(difference(&golden, &drawn)),
                Err(_) => State::New,
            };
            (g, drawn, state)
        })
        .collect();
    let expected: Vec<String> = GOLDENS
        .iter()
        .map(|g| format!("{}.png", g.name))
        .chain([VERSION_FILE.to_string()])
        .collect();
    let orphans = std::fs::read_dir(&dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            let name = p.file_name().expect("a directory entry has a name");
            !expected.contains(&name.to_string_lossy().into_owned())
        })
        .collect();
    let recorded = std::fs::read_to_string(dir.join(VERSION_FILE))
        .ok()
        .and_then(|s| s.trim().parse().ok());
    Survey {
        recorded,
        cases,
        orphans,
    }
}

fn bless(s: &Survey) -> Result<(), String> {
    let changed: Vec<&str> = s
        .cases
        .iter()
        .filter(|(_, _, st)| matches!(st, State::Changed(_)))
        .map(|(g, _, _)| g.name)
        .collect();
    if !changed.is_empty() && s.recorded == Some(render::VERSION) {
        return Err(format!(
            "{changed:?} changed but render::VERSION is still {}. Bump it in src/render.rs \
             so the engine redraws the tiles it has cached, then bless again.",
            render::VERSION
        ));
    }
    let dir = golden_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    for (g, drawn, st) in &s.cases {
        if !matches!(st, State::Same) {
            std::fs::write(dir.join(format!("{}.png", g.name)), drawn)
                .map_err(|e| e.to_string())?;
        }
    }
    for p in &s.orphans {
        std::fs::remove_file(p).map_err(|e| e.to_string())?;
    }
    std::fs::write(dir.join(VERSION_FILE), format!("{}\n", render::VERSION))
        .map_err(|e| e.to_string())
}

fn problems(s: &Survey) -> Vec<String> {
    let mut out = Vec::new();
    if s.recorded != Some(render::VERSION) {
        let at = s
            .recorded
            .map_or("no version".to_string(), |v| v.to_string());
        out.push(format!(
            "render::VERSION is {} but the goldens were drawn at {at}",
            render::VERSION
        ));
    }
    for (g, drawn, st) in &s.cases {
        let why = match st {
            State::Same => continue,
            State::Changed(d) => d.clone(),
            State::New => "no golden".to_string(),
        };
        let look = std::env::temp_dir().join(format!("omahelm-golden-{}.png", g.name));
        let _ = std::fs::write(&look, drawn);
        out.push(format!("{}: {why} (drawn: {})", g.name, look.display()));
    }
    for p in &s.orphans {
        out.push(format!("{}: no case in GOLDENS", p.display()));
    }
    out
}

#[test]
fn tiles_match_their_goldens() {
    let s = survey();
    if std::env::var_os("BLESS").is_some() {
        bless(&s).unwrap();
        return;
    }
    let problems = problems(&s);
    assert!(
        problems.is_empty(),
        "tests/golden/ is out of date:\n  {}\nIf the change is intended, run `mise bless` and \
         say in the PR what changed visibly.",
        problems.join("\n  ")
    );
}
