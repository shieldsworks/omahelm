# Days sailed

Every day the logbook has a track for, each put back together from the
passages omalogbook wrote: recorded runs solid, inferred gaps dashed, from
a terminal or over the engine's socket for the window's trips layer.

## Sub-features

- `trips-list` `omahelm trips` prints a line per day with its run, passages, gaps and hours
- `trip-gpx` `omahelm trip --date D --out FILE` writes the day as one GPX track with its holes closed
- `trips-missing` a logbook folder that isn't there is an error naming the folder
- `trips-socket` the engine answers `trips` with the days and `trip` with a day's lines

## How to get to it (user POV)

- Run `omahelm trips [--logbook DIR]` or `omahelm trip [--date YYYY-MM-DD] [--logbook DIR] [--out FILE.gpx]`.
- In the window, `p` draws every day and `d` opens the calendar; both ask the engine.

## Driving it with the CLI and the socket

Preconditions:

- The baseline from `README.md`: `tests/fixtures/logbook` copied to `$run/logbook`, named by `logbook` in `$run/config.toml`, and `TZ=UTC` in `$run/env`.
- For **Over the socket**, the engine started per `../SKILL.md`.

- **The days.** Run `omahelm trips`. Stdout `2026-09-21     7.4 nm   2 passages   1 gap  (1.5 nm)  18:43–21:14`, stderr `1 day, 7.4 nm, in $run/logbook`, exit 0.
- **One day as GPX.** Run `omahelm trip --date 2026-09-21 --out "$run/artifacts/verify/trip-gpx/day.gpx"`. Stderr `2026-09-21: 2 passages joined, 1 gap filled with straight lines, 1.5 of 7.4 nm inferred`, exit 0. A second look: the file is GPX with one `<trk>` and 46 recorded `<trkpt>` lines.
- **No logbook.** Run `omahelm trips --logbook "$run/nowhere"`. Stderr `omahelm: no tracks at $run/nowhere. Set \`logbook\` in config.toml, or pass --logbook.`, exit 1.
- **Over the socket.** Run `$h $s --send '{"type":"trips"}' --until type=trips`: `"status":"ok"`, one day `"date":"2026-09-21"` with `"passages":2`, `"holes":1`, `"points":46`. Then `$h $s --send '{"type":"trip","id":3,"date":"2026-09-21","z":13}' --until last=true`: one `trip` message with `"id":3`, `"last":true`, `"days":1`, one entry in `gaps`.
- **Proof.** Save stdout, stderr, exit codes, the GPX and the socket transcripts to `$run/artifacts/verify/<id>/`; `git status --porcelain` in the checkout is unchanged, since the logbook is only read.

## Gotchas

- Hours in `omahelm trips` are local time; without `TZ=UTC` they shift with the machine's zone. Dates come from the file names, so they don't.
- The logbook belongs to omalogbook. Drive a copy in `$run`, never `~/Logbook`.
- Without `logbook` in the config, omahelm asks omalogbook's own config for the vault; `$run/env` points `XDG_CONFIG_HOME` into the run so that lookup finds nothing of the user's.
