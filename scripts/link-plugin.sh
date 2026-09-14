#!/usr/bin/env bash
# Points the Omarchy shell's plugin at this checkout, for development.
# The installed plugin never runs this.
#   --link (default)  symlink ~/.config/omarchy/plugins/org.omahoy.helm here
#   --unlink          remove the link, restoring an installed copy if one was kept
#   --status          say which copy the shell loads
set -euo pipefail
cd "$(dirname "$0")/.."

die() { printf '%s\n' "$@" >&2; exit 1; }

id=org.omahoy.helm
root=$(readlink -f "$PWD")
# The shell scans ~/.config/omarchy/plugins whatever XDG_CONFIG_HOME says.
plugins=${HOME:?}/.config/omarchy/plugins
target=$plugins/$id
backup=$plugins/.$id.unlinked

[[ -f $root/manifest.json ]] || die "Not an omahelm checkout: no $root/manifest.json"
[[ $(jq -r .id "$root/manifest.json") == "$id" ]] || die "manifest.json is not $id"

linked() { [[ -L $target && $(readlink -f "$target") == "$root" ]]; }
shell_up() { command -v omarchy-shell >/dev/null 2>&1 && omarchy-shell shell ping >/dev/null 2>&1; }

# The shell caches QML loaded through the old path, so it restarts.
restart_shell() {
  shell_up || return 0
  omarchy restart shell >/dev/null 2>&1 || true
  for _ in $(seq 50); do shell_up && return 0; sleep 0.1; done
}

enable() {
  shell_up || { printf 'Enable it once the shell runs: omarchy plugin enable %s\n' "$id"; return 0; }
  for _ in $(seq 40); do
    omarchy-shell shell listPlugins 2>/dev/null | jq -e --arg id "$id" 'any(.[]; .id == $id)' >/dev/null && break
    sleep 0.05
  done
  [[ $(omarchy-shell shell enablePlugin "$id" '{}') == ok ]] || die "Could not enable $id"
  printf 'Enabled %s. Open it with: omarchy-shell shell toggle %s {}\n' "$id" "$id"
}

case ${1:---link} in
  --link)
    mkdir -p -- "$plugins"
    if ! linked; then
      [[ -L $target ]] && die "$target links elsewhere: $(readlink -f "$target"). Unlink it first."
      if [[ -e $target ]]; then
        [[ -e $backup || -L $backup ]] && die "A kept copy is already at $backup. Remove it and retry."
        mv -- "$target" "$backup"
        printf 'Kept the installed copy at %s\n' "$backup"
      fi
      ln -s -- "$root" "$target"
      printf 'Linked %s -> %s\n' "$target" "$root"
    fi
    restart_shell
    enable
    ;;
  --unlink)
    linked || die "$target is not a link to this checkout"
    rm -f -- "$target"
    if [[ -d $backup ]]; then
      mv -- "$backup" "$target"
      printf 'Restored the installed copy at %s\n' "$target"
    else
      printf 'Unlinked. Install with: omarchy plugin add https://github.com/shieldsworks/omahelm.git --enable\n'
    fi
    restart_shell
    ;;
  --status)
    if linked; then echo "this checkout (linked)"; else echo "installed copy"; fi
    ;;
  *) die "usage: link-plugin.sh [--link|--unlink|--status]" ;;
esac
