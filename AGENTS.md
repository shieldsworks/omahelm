# Working on omahelm

omahelm is part of Omahoy, a suite of small Omarchy apps for sailors, each in
its own repo under github.com/shieldsworks. It is the chartplotter: it reads
NOAA's electronic navigational charts (S-57 cells) and draws them itself.
`omahelm serve` is the chart engine. It indexes the charts, draws 256-pixel
tiles into a cache and answers what's charted at a point, over the Unix
socket in `docs/protocol.md`. The Quickshell window in `ui/` is its client:
it places the tiles and draws the boat and AIS from omakeel, the wind from
omawind, the stream from omatide and the days sailed from omalogbook. The
same binary's subcommands (`render`, `fetch`, `import`, `index`, `query`,
`trips`, `trip`, `dump`) work without the engine or the window.

## Toolchain

Run `mise install` first. `mise.toml` pins the Rust toolchain (with rustfmt
and clippy); `Cargo.toml`'s `rust-version` is the minimum the code promises,
and clippy reads it as the MSRV. A system cargo older than that fails.
`mise tasks` lists every job.

`omahelm fetch` needs `curl` and `bsdtar`. The window needs Quickshell on a
Hyprland session (Omarchy); CI and cloud boxes have neither.

## Done means verified

You are not done until this passes from the repo root:

```sh
scripts/verify.sh
```

It runs, in order: `mise lint` (rustfmt check, clippy on all targets with
warnings denied), `mise test`, the comment check, the layout check, and a
check that verification left no new files in the tree. CI runs the same
script on x86_64 and aarch64, and qmllint 6.8.2 on `ui/`. Fix what it
reports; never weaken the check that reported it.

- `mise test` runs the unit tests, the S-57 reader against GDAL's counts in
  `tests/fixtures/*.gdal.txt`, and the golden tiles.
- `mise goldens` runs only the golden tiles: each case in `GOLDENS` in
  `tests/goldens.rs`, drawn from a temp copy of the fixtures and compared
  byte for byte with `tests/golden/<name>.png`. They are the same bytes on
  both architectures.
- `mise bless` redraws `tests/golden/` on purpose. It refuses when tiles
  changed and `render::VERSION` did not; bump it, so the engine redraws the
  tiles users have cached.

To prove behavior end to end (the app running, driven the way a user
drives it), use the verify skill: `.cursor/skills/verify/SKILL.md` and its
feature map in `.cursor/skills/verify/features/`.

## The gates are not yours to move

These files set the rules and are changed only in a PR whose whole purpose
is changing them, reviewed by a human:

- `[lints]` in `Cargo.toml`, and `clippy.toml`
- `.github/workflows/`, `scripts/verify.sh`, `scripts/check-comments.sh`
- `mise.toml` task definitions for `lint`, `test`, `goldens` and `bless`
- the comparison in `tests/goldens.rs` (adding a case to `GOLDENS` is a
  normal change)

The `[lints]` table is the suite's without `clippy::pedantic`, `expect_used`
and the `cast_*` lints. CI denies warnings, so they would fail the build at
hundreds of sites; they land together in their own PR, not one site at a
time.

To silence one lint at one site, use
`#[expect(clippy::<lint>, reason = "<the fact that makes this correct>")]` on
the smallest item. `#[allow]` without a reason is a CI error, and `#[expect]`
fails once the code stops needing it. A crate-level allow is never the fix,
with one exception: each `tests/*.rs` file is a crate of its own and
starts with the `#![allow(clippy::unwrap_used, reason = ...)]` that
`tests/charts.rs` has, because `allow-unwrap-in-tests` covers `#[test]`
functions and not their helpers.

## Every behavior change has a test or a golden

- A bug fix starts with a failing test that reproduces the bug.
- New behavior lands with the test that pins it. For output a test can't
  easily assert (rendered tiles, wire messages), add or update a golden and
  say in the PR why it changed.
- Never edit a golden by hand and never regenerate one to make a failing
  test pass without explaining the visible difference.
- Refactors change no test expectations. If one has to change, it was not
  a refactor.
- Tests write only to a temp dir, never into `tests/fixtures/` or the
  checkout. `Library::open` writes `index.json` into the chart root, so
  tests open charts with `common::Charts::open()` (`tests/common/mod.rs`),
  which copies the fixtures first.

## No apologetic comments

Comments state facts about the code and the world it handles ("NOAA ships
cells as ISO 8211, so ..."). They do not apologize, defer or hedge. These
fail CI (`scripts/check-comments.sh`): TODO, FIXME, XXX, HACK, workaround,
temporary fix, quick fix, for the time being, not ideal, should be fixed,
sorry, kludge, band-aid. If something is wrong, fix it in this change or
open an issue and leave the code honest. A workaround for an outside bug is
written as the fact: what the outside thing does and what this code does
about it.

## Copy the right pattern

You will copy what you see. Before copying, check that the code you copy
passes today's lints and has a test; old code may predate the rules. In
particular:

- No `unwrap()` outside tests. Return a `Result` with context, or
  `expect("<invariant that guarantees this>")` when it truly cannot fail.
- Every `unsafe` block carries a `// SAFETY:` comment saying why it is
  sound. Unsafe lives in two places: `trips::local` (`localtime_r`) and the
  signal handlers in `server::serve`. Don't add more without need.
- Numeric casts: use `try_from`, `From`, or a named helper that documents the
  range. A bare `as` that can truncate needs a reason.
- Take the shortcut only if it is also the right path. If the right path is
  hard, say so in the PR instead of shipping the shortcut.

## Review: the builder never approves its own work

- The agent that wrote a change does not approve, merge, or mark it
  verified. A different agent (fresh context, clean checkout) or Casey
  reviews it and runs `scripts/verify.sh` plus the verify skill for the
  features the change touches.
- The PR description lists: what changed, the tests or goldens that prove
  it, which feature-map entries were driven, and what was not checked.
- The reviewer reports what it ran and saw, not what it assumes.

## Suite conventions

- Rust, edition 2024, written from scratch with few dependencies: `serde`,
  `serde_json`, `libc`, `tiny-skia`, `png`, `ttf-parser`. Ask before adding
  a crate.
- Always pass `--locked`. Don't change `Cargo.lock` unless the task is a
  dependency change.
- Apps talk over local sockets. The engine's wire format is specified in
  `docs/protocol.md`. Change the doc in the same PR as the code.
- Fixtures in `tests/fixtures/` are real published data or documented
  synthetic data; don't edit them by hand. `tests/fixtures/README.md` says
  where each came from.
- The QML window in `ui/` runs under Quickshell as an Omarchy plugin
  (`manifest.json`). Keep it in step with the engine's protocol.

## Rules specific to omahelm

- The ISO 8211 and S-57 readers, the projection and the renderer are
  written from scratch. No GDAL, no PROJ, no map or tile library.
- A message the engine sends or accepts changes in `src/server.rs` and
  `docs/protocol.md` together, and in `ui/Helm.qml` when the window reads it.
- A change to how tiles look bumps `render::VERSION`; `mise bless` enforces
  it.
- Never pass `tests/fixtures` as `--charts`. Copy it to a temp dir.
- The trips layer only reads the logbook. omalogbook owns it.

## Layout

- `src/main.rs`: the CLI: arguments, the subcommands, and `render`'s
  compositing of tiles into one PNG
- `src/lib.rs`: the crate root; every module is public, for the CLI and the tests
- `src/server.rs`: the chart engine, `omahelm serve`, and its socket
- `src/render.rs`: one tile: cells coarse to fine, then areas, lines,
  symbols, names; `VERSION` names the drawing
- `src/style.rs`: the palette (Omarchy theme, paper, night) and the depth
  settings in `config.toml`
- `src/text.rs`: glyph outlines from a font file (`OMAHELM_FONT`, then
  fontconfig), filled as paths
- `src/marks.rs`: chart notation for lights, buoys, bottoms and colors
- `src/paths.rs`: `HOME` and the XDG base directories
- `src/chart.rs`: a cell projected to Web Mercator, ready to draw
- `src/library.rs`: the charts on disk: `index.json` and a cache of cells
- `src/fetch.rs`: NOAA downloads, and `import` of zips and folders
- `src/geo.rs`: Web Mercator, distances and rectangles
- `src/iso8211.rs`: the record format S-57 cells are written in
- `src/s57/`: the S-57 reader (`mod.rs`), the object catalogue
  (`codes.rs`, written by `scripts/gen-codes.py`) and the codes used by
  name (`names.rs`)
- `src/trips.rs`: the logbook's GPX tracks, a day at a time, and local time
- `ui/`: the Quickshell window; `shell.qml` standalone, `Panel.qml` as the
  plugin
- `docs/protocol.md`: the engine's socket protocol
- `tests/charts.rs`: the reader against GDAL, and a tile that isn't blank
- `tests/goldens.rs`, `tests/golden/`: the golden tiles
- `tests/common/mod.rs`: `Charts`, the fixtures opened from a temp copy
- `tests/fixtures/`: two NOAA cells, GDAL's counts, the golden font and a
  synthetic logbook
- `scripts/verify.sh`, `scripts/check-comments.sh`: the done command and
  the comment ban
- `scripts/link-plugin.sh`, `scripts/write-desktop-entry.sh`: install helpers
- `.cursor/skills/verify/`: the verify skill and its feature map
