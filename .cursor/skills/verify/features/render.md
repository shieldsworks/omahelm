# Render a view

Render a chart view to a PNG from the command line, the same tiles the
window draws, in the paper or the night palette, with no engine and no
window running.

## Sub-features

- `render-paper` a view framed on one tile matches its golden in the paper palette
- `render-night` the same view with `--night` matches the night golden
- `render-view` an arbitrary view writes the same bytes every run
- `render-bad-args` a missing `--center` is an error with exit code 1

## How to get to it (user POV)

- Run `omahelm render --center LAT,LON --zoom Z [--size WxH] [--scale N] [--night] [--charts DIR] --out FILE.png` in a terminal.

## Driving it with the CLI

Preconditions:

- The baseline from `README.md`: charts in `$run/charts`, `palette = "paper"`, the pinned font.
- The center below is the middle of tile 15/5250/12654, the tile in `tests/golden/berkeley-breakwater-*.png`.

- **Paper.** Run `omahelm render --center 37.8705172,-122.3162842 --zoom 15 --size 256x256 --scale 2 --charts "$run/charts" --out "$run/artifacts/verify/render-paper/view.png"`. Exit 0, stderr `4 tiles in N ms, 1:13469`, and `sha256sum` equals that of `tests/golden/berkeley-breakwater-paper.png`.
- **Night.** The same command with `--night` and `.../render-night/view.png`. Its sha256 equals `tests/golden/berkeley-breakwater-night.png`'s.
- **Any view, twice.** Run `omahelm render --center 37.8683,-122.3205 --zoom 14 --size 512x512 --charts "$run/charts" --out "$run/artifacts/verify/render-view/a.png"`, then again to `b.png`. Exit 0 both times; the two files have the same sha256 and are 1024 × 1024 (`file` says so).
- **Bad arguments.** Run `omahelm render --zoom 14 --out "$run/x.png"`. Stderr `omahelm: --center LAT,LON is required`, exit 1, and no `$run/x.png`.
- **Proof.** Save each command's stdout, stderr, exit code and PNG to `$run/artifacts/verify/<id>/`.

## Gotchas

- `omahelm render --charts DIR` writes `DIR/index.json`. Point it at `$run/charts`, never at `tests/fixtures`; the clean-tree check in `scripts/verify.sh` reports the stray file.
- Without `$run/env`, `render` uses your Omarchy theme, your `config.toml` and whatever font fontconfig picks, so its bytes match no golden.
- A missing `OMAHELM_FONT` file is not an error: the font search falls through to fontconfig. Check the path exists before comparing hashes.
