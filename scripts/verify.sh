#!/usr/bin/env bash
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"

all_steps=(lint test comments layout engine clean)
steps=("$@")
((${#steps[@]})) || steps=("${all_steps[@]}")

failed=()
say() { printf '\n== %s\n' "$*"; }

before=$(git status --porcelain --untracked-files=all)

step_lint() { mise lint; }
step_test() { mise test; }
step_comments() {
  sha256sum -c <<'EOF' || return 1
f3ec8a2d07e8f1e7c50770d0bb93579fc7bb0d60fc8e359a22607ba55cebc6c6  scripts/check-comments.sh
EOF
  scripts/check-comments.sh
}

step_layout() {
  local p bad=0 named
  named=$(sed -n '/^## Layout/,$ s/^- `\([^`]*\)`.*/\1/p' AGENTS.md)
  for p in src/*.rs src/*/ ui/; do
    grep -qxF "$p" <<< "$named" || { printf 'AGENTS.md Layout does not name %s\n' "$p"; bad=1; }
  done
  while IFS= read -r p; do
    [[ -e $p ]] || { printf 'AGENTS.md Layout names %s, which does not exist\n' "$p"; bad=1; }
  done <<< "$named"
  return "$bad"
}

step_engine() {
  if ! pkg-config --exists Qt6Core Qt6Gui Qt6Qml Qt6Network; then
    echo 'Qt6 QML dev packages not installed. Skipped.'
    return 0
  fi

  local repo_expr start_script work
  repo_expr=$(sed -n 's/^[[:space:]]*readonly property string repo:[[:space:]]*//p' ui/Helm.qml)
  start_script=$(awk -F\' '/var script = / { print $2; exit }' ui/Helm.qml)
  [[ -n $repo_expr && -n $start_script ]] || {
    echo 'ui/Helm.qml is missing repo or the start script' >&2
    return 1
  }
  [[ $(printf '%s\n' "$repo_expr" | wc -l) -eq 1 ]] || {
    echo 'repo must be one line' >&2
    return 1
  }

  work=$(mktemp -d)
  (
    trap 'rm -rf "$work"' EXIT
    export QT_QPA_PLATFORM=offscreen
    export XDG_RUNTIME_DIR=${XDG_RUNTIME_DIR:-/tmp/runtime-omahelm-verify}
    mkdir -p "$XDG_RUNTIME_DIR"
    chmod 700 "$XDG_RUNTIME_DIR"

    g++ -fPIC -std=c++17 scripts/qs-url-probe.cpp -o "$work/qs-url-probe" \
      $(pkg-config --cflags --libs Qt6Core Qt6Gui Qt6Qml Qt6Network)

    cat > "$work/probe.qml" <<EOF
import QtQuick
QtObject {
    readonly property string repo: ${repo_expr}
    Component.onCompleted: {
        console.log("REPO=" + repo)
        Qt.quit()
    }
}
EOF

    failed=0
    check() {
      local name=$1 file=$2 expect=$3
      local root=${4:-}
      local out got ran runtime log
      if [[ -n $root ]]; then
        out=$("$work/qs-url-probe" "$file" "$root" 2>&1) || true
      else
        out=$("$work/qs-url-probe" "$file" 2>&1) || true
      fi
      got=$(printf '%s\n' "$out" | sed -n 's/^qml: REPO=//p' | tail -n 1)
      runtime=$work/rt
      rm -rf "$runtime"
      mkdir -p "$runtime"
      log=$runtime/engine.log
      : > "$work/ran"
      RECORD=$work/ran PATH=/usr/bin:/bin \
        sh -c "$start_script" omahelm-start "$runtime" "$got/target/release/omahelm" "$log" || true
      ran=$(cat "$work/ran" 2>/dev/null || true)
      printf '\n== %s\n' "$name"
      printf '%s\n' "$out" | sed -n 's/^/  /p'
      printf '  repo: %s\n  want: %s\n  started: %s\n' "$got" "$expect" "${ran:-<none>}"
      if [[ -s $log ]]; then
        printf '  log: %s\n' "$(tail -n 1 "$log")"
      fi
      if [[ $got != "$expect" || $ran != "$expect/target/release/omahelm" ]]; then
        echo '  FAIL' >&2
        failed=1
      else
        echo '  ok'
      fi
    }

    mkdir -p "$work/checkout/ui" "$work/checkout/target/release"
    cat > "$work/checkout/target/release/omahelm" <<'EOF'
#!/bin/sh
printf '%s\n' "$0" > "$RECORD"
EOF
    chmod +x "$work/checkout/target/release/omahelm"
    cp "$work/probe.qml" "$work/checkout/ui/probe.qml"
    check 'checkout, quickshell -p ui/shell.qml' \
      "$work/checkout/ui/probe.qml" "$work/checkout"

    mkdir -p "$work/shell/plugins/org.omahoy.helm/ui" \
      "$work/shell/plugins/org.omahoy.helm/target/release"
    cp "$work/checkout/target/release/omahelm" \
      "$work/shell/plugins/org.omahoy.helm/target/release/omahelm"
    cp "$work/probe.qml" "$work/shell/plugins/org.omahoy.helm/ui/probe.qml"
    check 'plugin under the shell config root' \
      "$work/shell/plugins/org.omahoy.helm/ui/probe.qml" \
      "$work/shell/plugins/org.omahoy.helm" \
      "$work/shell"

    miss_log=$work/miss.log
    : > "$miss_log"
    PATH=/usr/bin:/bin \
      sh -c "$start_script" omahelm-start "$work/miss" "$work/missing/omahelm" "$miss_log" || true
    if [[ ! -s $miss_log ]] || ! grep -q omahelm "$miss_log"; then
      echo 'FAIL: a missing release binary did not fall back to omahelm on PATH' >&2
      failed=1
    else
      printf 'PATH fallback log: %s\n' "$(tail -n 1 "$miss_log")"
    fi

    if ! grep -q 'Quickshell.env("OMAHELM_BIN")' ui/Helm.qml; then
      echo 'FAIL: OMAHELM_BIN no longer overrides the binary' >&2
      failed=1
    fi
    if ! grep -q 'wrapMode: Text.Wrap' ui/ChartWindow.qml \
      || ! grep -q 'helm.lastLog' ui/ChartWindow.qml; then
      echo 'FAIL: the window does not wrap helm.lastLog when the engine is unreachable' >&2
      failed=1
    else
      echo 'window surfaces helm.lastLog'
    fi

    if ((failed)); then
      echo 'FAILED' >&2
      exit 1
    fi
    echo 'All engine-path checks passed.'
  )
}

step_clean() {
  local after
  after=$(git status --porcelain --untracked-files=all)
  if [[ $after != "$before" ]]; then
    printf 'Verification changed the tree. Tests write to a temp dir:\n'
    diff <(printf '%s\n' "$before") <(printf '%s\n' "$after") | sed -n 's/^> /  /p'
    return 1
  fi
  local stray found=0
  for stray in tests/fixtures/index.json tests/fixtures/index.json.tmp; do
    if [[ -e $stray ]]; then
      printf '%s\n' "$stray"
      found=1
    fi
  done
  return "$found"
}

for s in "${steps[@]}"; do
  [[ " ${all_steps[*]} " == *" $s "* ]] || { printf 'Unknown step: %s\n' "$s" >&2; exit 2; }
  say "$s"
  "step_$s" || failed+=("$s")
done

if ((${#failed[@]})); then
  printf '\nFAILED: %s\n' "${failed[*]}" >&2
  exit 1
fi
printf '\nAll verification steps passed.\n'
