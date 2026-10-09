#!/usr/bin/env bash
# Fails when a comment apologizes for the code instead of fixing it.
# Part of the Omahoy agent-ready template; CI runs it on every push.
#
# Banned: TODO, FIXME, XXX, HACK, "workaround", "temporary fix/hack/solution",
# "quick fix", "for the time being", "not ideal", "should be fixed",
# "sorry", "kludge", "band-aid". Fix the code, file an issue, or write down
# the constraint as a fact ("NOAA ships X, so Y").
#
# "for now" and "temporary file" are deliberately not banned: the suite uses "now" for the
# current time ("the nearest station's, for now").
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"

pattern='(^[[:space:]]*(#|\*|/\*|<!--)|//).*\b(TODO|FIXME|XXX|HACK|[Ww]orkaround|[Tt]emporary (fix|hack|solution)|[Qq]uick fix|[Ff]or the time being|[Nn]ot ideal|[Ss]hould be fixed|[Ss]orry|[Kk]ludge|[Bb]and-?aid)\b'

# Tracked source only; fixtures and vendored code are other people's words.
mapfile -t files < <(git ls-files -- '*.rs' '*.qml' '*.js' '*.sh' '*.py' '*.toml' '*.yml' \
  ':!:tests/fixtures/**' ':!:**/vendor/**' ':!:scripts/check-comments.sh')
((${#files[@]})) || exit 0

if hits=$(grep -HnE "$pattern" -- "${files[@]}"); then
  printf '%s\n' "$hits"
  printf '\nApologetic or deferred-work comments are not allowed (see AGENTS.md).\n' >&2
  exit 1
fi
