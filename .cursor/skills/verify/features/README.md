# omahelm verification map

This directory is the maintained source for verifying omahelm's user-facing
behavior. Read this index before driving the app, then use the matching
feature file as the recipe. Keep it current: a change that adds or alters a
user path updates its feature file in the same PR.

## Baseline preconditions

- A release build of this checkout and a `$run` made per `../SKILL.md`:
  the two fixture cells in `$run/charts/ENC_ROOT`, the synthetic logbook in
  `$run/logbook`, `palette = "paper"` in `$run/config.toml`, and `$run/env`
  pointing `HOME`, `XDG_*`, `OMAHELM_CONFIG` and `OMAHELM_FONT` into the run.
- For socket features, the engine started per `../SKILL.md` and Doctor
  passing.
- Never drive an instance this run did not start.

## Driving conventions

- Start every recipe from the baseline unless its preconditions say otherwise.
- Commands are literal; keep quoted names and flags unchanged. Every
  `omahelm` command runs as `env $(cat "$run/env") ./target/release/omahelm`,
  written `omahelm` below.
- Prefer stable handles: socket message types from `docs/protocol.md`, CLI
  subcommands and flags, tile keys, file paths. Not screen coordinates.
- Restore seeded data after a mutation. Don't delete proof during cleanup.

## Proof and skip reporting

- CLI proof: the command, stdout, stderr and exit code.
- Socket proof: the request line and the reply lines, verbatim.
- Image proof: the PNG plus its sha256, compared with `tests/golden/` when
  the recipe names a golden.
- Window proof (Casey's machine only): a `grim` screenshot with the app visible.
- Mutation proof: a read-only second look at what was stored.
- Record the feature ID and entry point with every artifact.
- An unreachable path is reported with the command tried and the unmet
  precondition, never as verified through a different path.

## Feature entry contract

Each feature file starts with an H1 title and one paragraph describing the
user-visible behavior, then exactly these four H2s, in order:

1. `Sub-features`: short IDs, one line each.
2. `How to get to it (user POV)`: every user entry point.
3. `Driving it with <harness>`: starts with `Preconditions:`, then labeled
   bullets pairing each user action with an exact command and the
   observable result.
4. `Gotchas`: traps that waste or invalidate a run.

Keep implementation details out of the map. Name user paths, stable
handles, required state, commands and observable proof.

## Features

- [Render a view](./render.md) covers `omahelm render` in the paper and
  night palettes, a view's determinism, and bad arguments.
- [The chart engine](./engine.md) covers `omahelm serve` over its socket:
  `hello` and `state`, `tiles` in both looks, `query`, a bad request, and a
  second engine.
- [Charts on disk](./charts.md) covers `omahelm import`, `omahelm index`
  with a cell it can't read, and `omahelm fetch --list`.
- [Days sailed](./trips.md) covers `omahelm trips`, `omahelm trip`, a
  missing logbook, and the engine's `trips` and `trip` messages.
- Not mapped: the window (`./run.sh`), which only Casey's machine can run.
