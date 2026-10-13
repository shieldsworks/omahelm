# The chart window

`./run.sh` opens the chartplotter. The window starts `omahelm serve`
itself when the socket is down, from the release binary next to this
checkout, and if that engine never answers it shows a notice instead of
a blank chart.

## Sub-features

- `window-launch` `./run.sh` opens the window under Quickshell
- `window-start` the window's start script runs this checkout's release binary, and the engine answers `hello`
- `window-error` a missing engine is named in the notice, with the last log line, and the chart does not stay blank
- `window-runtime` a bad `XDG_RUNTIME_DIR` is an error, and the window does not open a socket under `/tmp`

## How to get to it (user POV)

- Run `./run.sh` from the checkout. It sets `OMAHELM_BIN` to `target/release/omahelm` and execs `quickshell -p ui/shell.qml`.
- The Omarchy plugin loads the same window. It resolves the binary from the QML file's directory, the same way `quickshell -p` does.

## Driving it with the start script

Preconditions:

- The baseline from `README.md`, plus a release binary at `target/release/omahelm`.
- For **Start the engine**, copy the fixture cells to `$run/data/omahelm/charts` (the window passes no `--charts`) and stop any engine already on `$run/omahelm/helm.sock`.
- `h=.cursor/skills/verify/scripts/helm.py`.
- Quickshell and Hyprland are required for the painted window. This VM has neither. `window-launch` and the painted notice are unreachable here. The other steps are the substitutes that were run, and they are not a screenshot.

- **Open the window.** Run `./run.sh`. Observed exit 127 and stderr `./run.sh: line 12: exec: quickshell: not found`. The release binary was present, so the script got past its executable check. `quickshell` is not an Ubuntu package. The painted window was not driven.
- **Start the engine.** Run the script from `ui/Helm.qml`. Set `script='mkdir -p -m 700 "$1" && b="$2"; [ -x "$b" ] || b=omahelm; exec "$b" serve 2>>"$3"'` and `bin=$(realpath ./target/release/omahelm)`. Then run `env $(cat "$run/env") env -C "$run/home" sh -c "$script" omahelm-start "$run/omahelm" "$bin" "$run/omahelm/engine.log"` and record that pid. Observed the process command `/workspace/target/release/omahelm serve`. The log was `omahelm: serving $run/omahelm/helm.sock from $run/data/omahelm/charts`. Then `$h $run/omahelm/helm.sock --until charts.status=ok` printed `{"helm":"0.1.0","type":"hello","v":1}` and a `state` with `"cells":2`, `"status":"ok"`, and `charts.root` equal to `$run/data/omahelm/charts`. Exit 0.
- **Path from this file.** Run `scripts/verify.sh engine` where `pkg-config` can see Qt6 and QML can import `QtQuick` and `QtQml.WorkerScript`. The probe sets `QT_QPA_PLATFORM=offscreen`. Observed `repo` equal to the temp checkout and `started` equal to that checkout's `target/release/omahelm`. The plugin case resolved `repo` and `started` under `plugins/org.omahoy.helm`. The transcript then said `window surfaces helm.lastLog` and `All engine-path checks passed.`
- **Missing engine.** Run the same start script with `PATH=/usr/bin:/bin` and a binary path that is not executable. Observed exit 127 and log `omahelm-start: 1: exec: omahelm: not found`. That line is what the window stores as `helm.lastLog`. After six failed connects the notice text is `The chart engine isn't running.` then `Tried: <binary> serve` then that log line. The rectangle that draws it was not driven.
- **No runtime directory.** Run `node -e 'const fs=require("fs"); const {runtimeError}=new Function(fs.readFileSync("ui/runtime.js","utf8")+"\nreturn {runtimeError};")(); console.log(runtimeError(""));'`. Observed `XDG_RUNTIME_DIR must be set to an absolute path`. That string is the notice, and the window does not call the start script.
- **Proof.** Save the `./run.sh` stderr, the start-script log, the `helm.py` transcript, and the `scripts/verify.sh engine` transcript under `$run/artifacts/verify/window/`. Do not label them as a window screenshot.

## Gotchas

- `scripts/verify.sh engine` prints `Qt6 QML dev packages not installed. Skipped.` and exits 0 when `pkg-config` cannot see Qt6. That is not a pass of this feature. Without `qml6-module-qtquick` the probe prints `module "QtQuick" is not installed` and fails. Without `qml6-module-qtqml-workerscript` it prints `module "QtQml.WorkerScript" is not installed` and fails. This VM installed those packages so the probe could load. A Hyprland machine with Quickshell does not need them.
- The start script does not pass `--charts`. Cells belong in `$XDG_DATA_HOME/omahelm/charts`, never in `tests/fixtures`.
- Kill the pid you recorded. A second `omahelm serve` on the same socket exits 1 and is not the one to kill.
- `qmlpreview ui/shell.qml` is not a substitute. Here it exited 1 with `execvp: Permission denied` and drew nothing.
