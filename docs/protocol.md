# omahelm protocol

Version 1. The chart engine (`omahelm serve`) is the server. The chartplotter
window is a client. Chart pixels never travel over the socket: the engine
writes tiles as PNG files and names them.

## Transport

- A Unix stream socket, `$XDG_RUNTIME_DIR/omahelm/helm.sock`. The directory
  is created with mode 0700. A lock file beside it (`helm.sock.lock`) keeps a
  second engine from starting; a socket left by a crashed engine is replaced.
- Newline-delimited JSON, UTF-8, one object per line.
- Every engine message has `"type"` and `"v": 1`. A client that sees another
  `v` shows an error and stops using the engine.
- Key order is not significant. Keys that don't apply are left out, never
  sent as `null`. Clients ignore message types and keys they don't know.
- Any number of clients may connect. `state` goes to every client; `tile`
  and `features` go only to the client that asked.
- A line longer than 64 KiB from a client, or one that isn't a JSON object
  with a known `type`, is answered with an `error` and otherwise ignored.

## Engine messages

`hello` is sent once on connect, followed by `state`.

```json
{"type":"hello","v":1,"helm":"0.1.0"}
```

`state` is the complete current state, re-sent whenever it changes. Clients
replace their copy.

```json
{"type":"state","v":1,
 "charts":{"status":"ok","cells":387,"skipped":43,
           "root":"/home/casey/.local/share/omahelm/charts",
           "extent":{"west":-124.6,"south":32.3,"east":-117.1,"north":42.1}},
 "tiles":{"root":"/home/casey/.cache/omahelm/tiles/3f9a1c2e5d7b8a90/","generation":"3f9a1c2e5d7b8a90"},
 "settings":{"units":"feet","safetyDepth":10,"shallowContour":6,"safetyContour":12,
             "deepContour":30,"palette":"theme"},
 "problems":["config.toml: unknown setting bogus"]}
```

- `charts.status` is `indexing` while cells are read (with
  `progress: {"done": n, "total": m}`), `ok` with at least one chart, and
  `empty` with none. `extent` is present only with charts.
- `skipped` counts cells that couldn't be used, cancelled cells included;
  `omahelm index` lists them.
- `tiles.root` is an absolute directory ending in `/`. Tile paths are
  relative to it. It changes, with `generation`, whenever the drawing would:
  the theme, the settings, the charts, or the renderer. A client drops the
  tiles it holds when `generation` changes and asks again.
- `settings` are in `units`: the depths as the user wrote them in
  `~/.config/omahelm/config.toml`.
- `problems` are human-readable, for display. Absent when there are none.

`tile` answers a `tiles` request, one message per tile, nearest the centre
of the request first.

```json
{"type":"tile","v":1,"z":15,"x":5249,"y":12655,"scale":2,"generation":"3f9a1c2e5d7b8a90",
 "path":"15/5249/12655@2.png"}
```

- `path` is relative to `tiles.root` and has the form
  `<z>/<x>/<y>@<scale>.png`. The file is complete before the message is sent.
- `generation` is the look the tile was drawn in. A client ignores a tile
  whose `generation` isn't the one in its latest `state`.
- A tile that couldn't be drawn has `error` instead of `path`.
- Tiles for a request that was replaced are not sent, except those already
  on their way.

`features` answers a `query`: what's charted at a point, most specific first.

```json
{"type":"features","v":1,"id":7,"lat":37.8683,"lon":-122.3205,
 "features":[
  {"class":"BCNLAT","kind":"Lateral beacon","title":"Berkeley Marina Light 2",
   "label":"R \"2\"","lines":["Red","Light: Fl R 4s 4M, 7 m high"],"chart":"US5OAKFI"},
  {"class":"DEPARE","kind":"Depth area","title":"6 to 12 ft","lines":[],"chart":"US5OAKFI"}]}
```

- `id` is the query's.
- `class` is the S-57 acronym and `kind` its name. `title` is the feature's
  name or the best one-line description. `label` is present for aids to
  navigation. `lines` are further facts, already in the display units.
- `chart` names the cell the feature came from.
- An empty `features` list means nothing is charted there.

`error` reports a bad request.

```json
{"type":"error","v":1,"message":"tiles: at most 256 tiles per request"}
```

## Client messages

`tiles` asks for the tiles of a rectangle at one zoom level and pixel scale.
It replaces the client's previous `tiles` request.

```json
{"type":"tiles","z":15,"x0":5246,"y0":12652,"x1":5252,"y1":12658,"scale":2}
```

- `z` is 0 to 18. `x0` ≤ `x1` and `y0` ≤ `y1`, all within the zoom level's
  `2^z` tiles. At most 256 tiles.
- `scale` is device pixels per logical pixel, 1 to 4. A tile is
  `256 × scale` pixels square.

`query` asks what's charted at a point, as seen at a zoom level: features
within about 10 logical pixels, and the areas that contain the point.

```json
{"type":"query","id":7,"lat":37.8683,"lon":-122.3205,"zoom":15}
```

## Files

- Charts: `$XDG_DATA_HOME/omahelm/charts/ENC_ROOT/<CELL>/<CELL>.000` and its
  updates, as NOAA ships them, with `index.json` beside `ENC_ROOT`.
- Tiles: `$XDG_CACHE_HOME/omahelm/tiles/<generation>/`. Older generations are
  deleted when the engine starts.
- Settings: `$XDG_CONFIG_HOME/omahelm/config.toml`. The engine re-reads it,
  and the Omarchy theme's `colors.toml`, when they change.
