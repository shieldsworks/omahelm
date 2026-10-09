# Charts on disk

Install ENC cells you already have, re-read the chart folder and see which
cells were skipped and why, and list the regions NOAA publishes.

## Sub-features

- `charts-import` `omahelm import DIR` installs every cell under DIR and re-indexes
- `charts-index` `omahelm index` counts the cells and lists any it skipped, with the reason
- `charts-fetch-list` `omahelm fetch --list` names NOAA's regions without downloading

## How to get to it (user POV)

- Run `omahelm import ZIP|DIR [--charts DIR]`, `omahelm index [--charts DIR]` or `omahelm fetch --list` in a terminal.
- `omahelm fetch REGION|CELL` downloads and then indexes; it needs NOAA's servers.

## Driving it with the CLI

Preconditions:

- The baseline from `README.md`, except that `$run/charts` starts empty: `rm -rf "$run/charts" && mkdir "$run/charts"`.

- **Import.** Run `omahelm import tests/fixtures/ENC_ROOT --charts "$run/charts"`. Stderr `Installed 2 cells in $run/charts`, stdout `2 cells in $run/charts`, exit 0. A second look: `find "$run/charts" -name '*.00?'` lists `US5OAKFG.000`, `.001`, `.002` and `US5OAKFI.000` under `ENC_ROOT/<CELL>/`, and `$run/charts/index.json` exists.
- **A cell it can't read.** Run `mkdir -p "$run/charts/ENC_ROOT/US5BROKN" && head -c 300 tests/fixtures/ENC_ROOT/US5OAKFI/US5OAKFI.000 > "$run/charts/ENC_ROOT/US5BROKN/US5BROKN.000"`, then `omahelm index --charts "$run/charts"`. Stdout `2 cells in $run/charts` and `  skipped ENC_ROOT/US5BROKN/US5BROKN.000: ...: field 0000 runs past the end of the file`, exit 0.
- **Regions.** Run `omahelm fetch --list`. The first lines are `AK  Alaska`, `AL  Alabama`, `CA  California`; exit 0; nothing under `$run` changes.
- **Proof.** Save each command's stdout, stderr and exit code, and the `find` listing, to `$run/artifacts/verify/<id>/`.

## Gotchas

- `import` reads the source and writes only `--charts`. Importing from `tests/fixtures` is fine; indexing it is not, because `index` writes `index.json` into the folder it reads.
- `index` re-reads only cells whose size or mtime changed, so its progress line counts the changed cells, not all of them.
- `fetch REGION|CELL` without network fails at `curl`; report it as not driven rather than as failed.
