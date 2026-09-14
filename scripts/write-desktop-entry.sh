#!/usr/bin/env bash
# Adds Omahelm to the app launcher: writes
# $XDG_DATA_HOME/applications/omahelm.desktop, which opens the shell's
# panel. Nothing else runs this; you do, once, if you want the entry.
set -euo pipefail

apps=${XDG_DATA_HOME:-${HOME:?}/.local/share}/applications
desktop=$apps/omahelm.desktop
mkdir -p -- "$apps"
tmp=$(mktemp -- "$apps/omahelm.desktop.XXXXXX")
trap 'rm -f -- "$tmp"' EXIT
cat > "$tmp" <<'EOF'
[Desktop Entry]
Type=Application
Name=Omahelm
GenericName=Chartplotter
Comment=NOAA charts drawn from scratch, with your boat and AIS
Exec=omarchy-shell shell toggle org.omahoy.helm {}
TryExec=omarchy-shell
Icon=compass
Terminal=false
StartupNotify=false
Categories=Education;Geography;
Keywords=chart;chartplotter;nautical;sailing;navigation;ENC;
EOF
chmod 644 -- "$tmp"
mv -f -- "$tmp" "$desktop"
trap - EXIT
command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database "$apps" >/dev/null 2>&1 || true
printf '%s\n' "$desktop"
