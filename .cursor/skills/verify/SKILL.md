---
name: verify-omahelm
description: "Drive omahelm the way a user does (the render CLI, the chart engine's socket, chart import and index, the trips commands) and capture proof. Use before calling any behavior change done, when reviewing someone else's change, or when asked to verify omahelm."
---

# Verify omahelm

`scripts/verify.sh` proves the code is sound (lint, tests, goldens). This
skill proves the app works: it runs the real binary, drives a feature from
the user's side and keeps evidence. Run both. The agent that built a change
is not the one that signs off on this run.

## Launch

Build first, in your own environment. The run's environment goes in
`$run/env` and is passed to each command with `env`, so cargo and mise
keep your real `HOME`.

```sh
mise install && cargo build --release --locked
run=$(mktemp -d /tmp/omahelm-verify.XXXXXX)
mkdir -p "$run/charts" "$run/home" "$run/artifacts/verify"
cp -r tests/fixtures/ENC_ROOT "$run/charts/"
cp -r tests/fixtures/logbook "$run/logbook"
printf 'palette = "paper"\nlogbook = "%s"\n' "$run/logbook" > "$run/config.toml"
cat > "$run/env" <<EOF
HOME=$run/home
XDG_RUNTIME_DIR=$run
XDG_CACHE_HOME=$run/cache
XDG_CONFIG_HOME=$run/config
XDG_DATA_HOME=$run/data
OMAHELM_CONFIG=$run/config.toml
OMAHELM_FONT=$PWD/tests/fixtures/fonts/DejaVuSansMono.ttf
TZ=UTC
EOF
```

The CLI needs nothing else: each drive is its own process,
`env $(cat "$run/env") ./target/release/omahelm ...`.

The engine, for the socket features:

```sh
env $(cat "$run/env") ./target/release/omahelm serve --charts "$run/charts" \
  >"$run/engine.log" 2>&1 & echo $! > "$run/pid"
.cursor/skills/verify/scripts/helm.py "$run/omahelm/helm.sock" --until charts.status=ok
```

Ready when that prints `hello`, then a `state` with `"status":"ok"` and
`"cells":2`, and exits 0.

## Doctor

```sh
realpath target/release/omahelm                  # a build inside this checkout
kill -0 "$(cat "$run/pid")" && test -S "$run/omahelm/helm.sock"
grep -c . "$run/env"                             # 8 lines, every path under $run
```

It must show this checkout's build, a live process this run started, and a
socket inside `$run`. Never drive the user's own running instance, their
real config, theme, logbook or chart folder.

## Drive

Follow the feature map in `features/`: start at `features/README.md`. A
proof that drives one convenient entry point is incomplete when the map
lists others.

- The CLI: `env $(cat "$run/env") ./target/release/omahelm <subcommand>`.
- The engine's socket: `scripts/helm.py`, below, speaking `docs/protocol.md`.
- The window (`./run.sh` under Quickshell, `grim` for screenshots) only on
  Casey's machine. CI and cloud boxes can't run it; report it as not driven.

## Evidence

Put everything in `$run/artifacts/verify/<feature-id>/`, then copy it to
`${EVIDENCE_DIR:-/tmp/omahelm-evidence}/<feature-id>/` before cleanup.

- Capture the action and the resulting state: the command or socket
  request, stdout, stderr, exit code, and the reply or file it produced.
- Check side effects with a second, read-only look (the file on disk, the
  next socket message), not just the first reply.
- Exercise the real user path. No test-only endpoints, no internal setters.
  The only stand-ins are the fixtures: two NOAA cells and a synthetic
  logbook.
- `omahelm fetch REGION|CELL` downloads from NOAA. Drive it only when the
  task is about fetching and the network is allowed; otherwise report it
  as not driven.
- Report anything you could not reach, with the command tried and the
  missing precondition. A skipped entry point is never reported as passed.

## Cleanup

```sh
kill "$(cat "$run/pid")" 2>/dev/null || true    # only what this run started, never pkill by name
rm -rf "$run/omahelm" "$run/cache" "$run/charts" "$run/logbook" "$run/home" "$run/config"
```

Evidence stays. Check it still exists after cleanup, and that
`git status --porcelain` in the checkout is what it was before the run.

## Helpers

`scripts/helm.py SOCKET [--send JSON]... --until KEY=VALUE [--timeout S]`
connects as the window does, prints every engine line verbatim and each
request it sent as `> ...`, and exits 0 at the first message whose `KEY`
(a dotted path such as `charts.status`) equals `VALUE`, or 1 after the
timeout (20 s). Requests go after the first `state`.
