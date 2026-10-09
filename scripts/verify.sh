#!/usr/bin/env bash
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"

all_steps=(lint test comments layout clean)
steps=("$@")
((${#steps[@]})) || steps=("${all_steps[@]}")

failed=()
say() { printf '\n== %s\n' "$*"; }

before=$(git status --porcelain --untracked-files=all)

step_lint() { mise lint; }
step_test() { mise test; }
step_comments() { scripts/check-comments.sh; }

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
