# The chart engine

`omahelm serve` indexes the charts, draws tiles into a cache for the window
and answers what's charted at a point, over the Unix socket specified in
`docs/protocol.md`. The window is one client; any number may connect.

## Sub-features

- `engine-hello` a client gets `hello`, then `state` with the charts' status, count and extent
- `engine-tiles` a `tiles` request gets a `tile` message naming a PNG that is already on disk
- `engine-night` the same request with `"look":"night"` names a tile drawn red on black
- `engine-query` a `query` gets `features` at a point, most specific first
- `engine-error` an unknown request gets an `error` and the connection stays open
- `engine-single` a second `omahelm serve` on the same socket says it is already running

## How to get to it (user POV)

- The window starts `omahelm serve` when it isn't running and connects to `$XDG_RUNTIME_DIR/omahelm/helm.sock`.
- Run `omahelm serve [--charts DIR]` in a terminal.

## Driving it with the socket

Preconditions:

- The baseline from `README.md` and the engine started per `../SKILL.md`; Doctor passes.
- `h=.cursor/skills/verify/scripts/helm.py` and `s=$run/omahelm/helm.sock`.

- **Connect.** Run `$h $s --until charts.status=ok`. The first line is `{"helm":"0.1.0","type":"hello","v":1}`; a `state` follows with `"cells":2`, `"skipped":0` and `charts.root` equal to `$run/charts`.
- **A tile.** Run `$h $s --send '{"type":"tiles","z":15,"x0":5250,"y0":12654,"x1":5250,"y1":12654,"scale":2}' --until type=tile`. The `tile` message has `"path":"15/5250/12654@2.png"`; that file under `tiles.root` from `state` exists, and its sha256 equals `tests/golden/berkeley-breakwater-paper.png`'s.
- **Night.** The same request with `,"look":"night"` before the closing brace. The file under `tiles.night.root` has the sha256 of `tests/golden/berkeley-breakwater-night.png`.
- **What's here.** Run `$h $s --send '{"type":"query","id":7,"lat":37.868315,"lon":-122.320472,"zoom":15}' --until type=features`. `"id":7`, and the first feature is `"class":"BCNLAT"`, `"title":"Berkeley North Breakwater Light 4"`, `"label":"R \"4\""`, `"chart":"US5OAKFI"`.
- **A bad request.** Run `$h $s --send '{"type":"bogus"}' --until type=error`. The reply is `{"message":"unknown type bogus","type":"error","v":1}`.
- **One engine.** Run `env $(cat "$run/env") ./target/release/omahelm serve --charts "$run/charts"`. It prints `omahelm: already running on $run/omahelm/helm.sock` and exits 0; the first engine still answers **Connect**.
- **Proof.** Save every `helm.py` transcript, the tile PNGs and their sha256, and `$run/engine.log` to `$run/artifacts/verify/<id>/`.

## Gotchas

- The socket path comes from `XDG_RUNTIME_DIR`; without `$run/env` you reach the user's own engine, if one runs.
- Tile hashes match the goldens only with `palette = "paper"` (the day look) and the pinned font. A different theme changes `tiles.root` and every day tile.
- `state` is re-sent while the charts are read; wait for `charts.status=ok` before asking for tiles.
- The engine writes `index.json` into `--charts`; never start it on `tests/fixtures`.
