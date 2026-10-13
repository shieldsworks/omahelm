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

- **Paper.** Run `omahelm render --center 37.8705172,-122.3162842 --zoom 15 --size 256x256 --scale 2 --charts "$run/charts" --out "$run/artifacts/verify/render-paper/view.png"`. Exit 0. Observed stderr `4 tiles in 18 ms, 1:13469` (the milliseconds change, the scale does not) and sha256 `7e5fc8bee2646afa0ca4e3fc38a1ff1dd767521368a7ff20b10737ade28f468c`, the same as `tests/golden/berkeley-breakwater-paper.png`.
- **Night.** The same command with `--night` and `.../render-night/view.png`. Observed stderr `4 tiles in 17 ms, 1:13469` and sha256 `1791c74240b46bf6e3b90ee139180502498cf36ac131064ad23e2cb50c3532e7`, the same as `tests/golden/berkeley-breakwater-night.png`.
- **Any view, twice.** Run `omahelm render --center 37.8683,-122.3205 --zoom 14 --size 512x512 --charts "$run/charts" --out "$run/artifacts/verify/render-view/a.png"`, then again to `b.png`. Exit 0 both times. Observed stderr `9 tiles in 30 ms, 1:26938`. Both files had sha256 `e45a0de7f0eaef069805597af8f3fbd6c9cd9b9a81f7780885279fcfa792e20c`. `file` said `PNG image data, 1024 x 1024, 8-bit/color RGB, non-interlaced`.
- **Bad arguments.** Run `omahelm render --zoom 14 --out "$run/x.png"`. Observed stderr `omahelm: --center LAT,LON is required`, exit 1, and no `$run/x.png`.
- **Proof.** Save each command's stdout, stderr, exit code and PNG to `$run/artifacts/verify/<id>/`.

## Gotchas

- `omahelm render --charts DIR` writes `DIR/index.json`. Point it at `$run/charts`, never at `tests/fixtures`; the clean-tree check in `scripts/verify.sh` reports the stray file.
- Without `$run/env`, `render` uses your Omarchy theme, your `config.toml` and whatever font fontconfig picks, so its bytes match no golden.
- A missing `OMAHELM_FONT` file is not an error: the font search falls through to fontconfig. Check the path exists before comparing hashes.
- The first `render` or `index` of a copy prints `reading charts 2/2` on stderr, with a carriage return. The tile line after it is the result.
