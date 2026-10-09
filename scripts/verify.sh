#!/usr/bin/env bash
# The one command that says a change to omahelm is done. Agents run it
# before calling a task done; CI runs it, unchanged, on x86_64 and aarch64.
#
#   scripts/verify.sh              every step
#   scripts/verify.sh lint test    only the named steps
#
# The golden tiles are a cargo test target, so `mise test` checks them.
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"

all_steps=(lint test comments layout clean)
steps=("$@")
((${#steps[@]})) || steps=("${all_steps[@]}")

failed=()
say() { printf '\n== %s\n' "$*"; }

# Snapshot the tree first, so "clean" can tell files verification created
# from work in progress that was already there.
before=$(git status --porcelain --untracked-files=all)

step_lint() { mise lint; }
step_test() { mise test; }
step_comments() { scripts/check-comments.sh; }

# AGENTS.md's Layout names every module and directory under src/, and ui/,
# each on its own "- `path`" line, and names nothing that is gone.
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

step_clean() {
  local after
  after=$(git status --porcelain --untracked-files=all)
  if [[ $after != "$before" ]]; then
    printf 'Verification changed the tree. Tests write to a temp dir:\n'
    diff <(printf '%s\n' "$before") <(printf '%s\n' "$after") | sed -n 's/^> /  /p'
    return 1
  fi
  # Library::open writes index.json, via index.json.tmp, into the chart
  # folder it was given. A copy already in tests/fixtures does not change
  # git status.
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
