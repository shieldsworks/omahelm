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
 "tiles":{"root":"/home/casey/.cache/omahelm/tiles/3f9a1c2e5d7b8a90/","generation":"3f9a1c2e5d7b8a90",
          "night":{"root":"/home/casey/.cache/omahelm/tiles/9c04e7d1a2b3f586/","generation":"9c04e7d1a2b3f586"}},
 "settings":{"units":"feet","safetyDepth":10,"shallowContour":6,"safetyContour":12,
             "deepContour":30,"palette":"theme"},
 "problems":["config.toml: unknown setting bogus"]}
```

- `charts.status` is `indexing` while cells are read (with
  `progress: {"done": n, "total": m}`), `ok` with at least one chart, and
  `empty` with none. `extent` is present only with charts.
- `skipped` counts cells that couldn't be used, canceled cells included;
  `omahelm index` lists them.
- `tiles.root` is an absolute directory ending in `/`. Tile paths are
  relative to it. It changes, with `generation`, whenever the drawing would:
  the theme, the settings, the charts, or the renderer. A client drops the
  tiles it holds when `generation` changes and asks again.
- `tiles.night` is the same for the Night Watch look: red on black whatever
  the theme. It changes with the settings, the charts and the renderer, but
  not the theme. An engine without it draws only the theme's look.
- `settings` are in `units`: the depths as the user wrote them in
  `~/.config/omahelm/config.toml`.
- `problems` are human-readable, for display. Absent when there are none.

`tile` answers a `tiles` request, one message per tile, nearest the center
of the request first.

```json
{"type":"tile","v":1,"z":15,"x":5249,"y":12655,"scale":2,"generation":"3f9a1c2e5d7b8a90",
 "path":"15/5249/12655@2.png"}
```

- `path` has the form `<z>/<x>/<y>@<scale>.png`. It is relative to the root
  of the look asked for: `tiles.root`, or `tiles.night.root` for a
  `"look":"night"` request. The two hold the same paths drawn differently.
  The file is complete before the message is sent.
- `generation` is the look the tile was drawn in. A client ignores a tile
  whose `generation` isn't the one it asked for in its latest `state`.
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

`trips` answers a `trips`: every day the logbook has a track for, oldest
first. The days themselves are drawn from `trip`.

```json
{"type":"trips","v":1,"status":"ok","root":"/home/casey/Logbook",
 "days":[{"date":"2026-09-21","passages":4,"points":1516,
          "distanceNm":20.2,"gapNm":2.45,"holes":3,"seconds":17260,
          "from":"2026-09-21T18:43:30Z","to":"2026-09-21T23:31:10Z",
          "bbox":{"west":-122.450883,"south":37.810592,
                  "east":-122.313158,"north":37.880113}}]}
```

- `status` is `ok` with at least one day, `empty` for a logbook with no
  usable track in it, and `none` when there is no logbook at `root`.
- `root` is the folder the days were read from.
- `date` is the local day the passages are filed under.
- `distanceNm` is the whole trip as it is drawn, the straight lines across
  the holes included. `gapNm` is how much of that was inferred and `holes`
  how many there were. `points` counts the fixes actually recorded.
- `from`, `to` and `seconds` are the day's first and last fix, in UTC. A
  day whose track carries no times at all has none of the three.
- `skipped` counts files that held no usable point. Absent when none did.

`trip` answers a `trip` with one day's lines, thinned for the zoom level
asked for. A range is answered a day at a time, as `tile` answers a
rectangle a tile at a time.

```json
{"type":"trip","v":1,"id":7,"date":"2026-09-21","z":13,"drawn":249,
 "runs":[[37.866705,-122.313328,37.86662,-122.313227]],
 "gaps":[[37.869063,-122.450883,37.82839,-122.449358]],
 "distanceNm":20.2,"gapNm":2.45,"holes":3,"passages":4,
 "bbox":{"west":-122.450883,"south":37.810592,
         "east":-122.313158,"north":37.880113},
 "last":true,"days":1}
```

- A **run** is track: a line of `lat, lon` pairs the receiver reported. A
  **gap** is the straight line between two runs. It is inferred, not
  sailed, and a client must draw it so that the two can't be confused.
- The day's other keys are the ones `trips` sends for it. `holes` is the
  count; `gaps` here are the lines across them.
- `drawn` counts the points left after thinning. One answer carries at
  most 120,000 points, shared between the days in it, so a season asked
  for at close range is thinned harder rather than sent whole. A line is
  never cut short: thinning drops points from the middle, never the ends.
- The last message of an answer carries `"last": true` and `days`, how
  many it sent. A request that matched nothing is that message alone,
  with no `date`. `"more": true` says days were left out: at most 500
  answer, the most recent of the range.
- `id` is the request's, echoed as it was sent.

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
- `look` is `"night"` for Night Watch tiles, or left out for the theme's.
  Each client chooses its own.

`query` asks what's charted at a point, as seen at a zoom level: features
within about 10 logical pixels, and the areas that contain the point.

```json
{"type":"query","id":7,"lat":37.8683,"lon":-122.3205,"zoom":15}
```

`trips` asks which days the logbook has tracks for. It takes nothing else.

```json
{"type":"trips"}
```

`trip` asks for a day's lines, or a range of days.

```json
{"type":"trip","id":7,"from":"2026-05-01","to":"2026-09-22","z":13}
```

- `date` for one day, or `from` and `to` for a range; either end may be
  left out, and with none of the three every day answers.
- Dates are `YYYY-MM-DD`: the local days `trips` named.
- `z` is 0 to 22, the zoom level the day is thinned for: a point within a
  third of a logical pixel of the line between its neighbors is dropped.
  Left out, the lines keep every point that changes their shape, short
  of the answer's budget.
- `id` is echoed on every message of the answer.

## Files

- Charts: `$XDG_DATA_HOME/omahelm/charts/ENC_ROOT/<CELL>/<CELL>.000` and its
  updates, as NOAA ships them, with `index.json` beside `ENC_ROOT`.
- Tiles: `$XDG_CACHE_HOME/omahelm/tiles/<generation>/`. Older generations are
  deleted when the engine starts.
- Settings: `$XDG_CONFIG_HOME/omahelm/config.toml`. The engine re-reads it,
  and the Omarchy theme's `colors.toml`, when they change.
- Tracks: `<logbook>/tracks/*.gpx`, written by
  [omalogbook](https://github.com/shieldsworks/omalogbook) and only read
  here. The folder is the `logbook` setting, else the `vault` omalogbook
  is configured for, else `~/Logbook`. A folder of GPX files named
  straight at the setting works too. The engine re-reads them whenever a
  `trips` or `trip` request comes in and a file has changed.
