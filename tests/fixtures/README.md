# Test fixtures

## Charts

Two NOAA electronic navigational charts (ENCs), as NOAA published them on
2026-09-11 at https://charts.noaa.gov/ENCs/. NOAA ENC data is in the public
domain. Not for navigation.

- `US5OAKFI`: San Francisco Bay, Emeryville. Edition 1, no updates. It holds
  Berkeley Marina's entrance.
- `US5OAKFG`: San Francisco Bay, Alcatraz and Angel Island. Edition 1 with
  updates 1 and 2, to test applying updates.

`<CELL>.gdal.txt` is each cell as GDAL 3.12.4 reads it, updates applied:
object class, feature count, vertex count. `tests/charts.rs` checks omahelm's
reader against it, class by class.

`omahelm` writes `index.json` into any chart folder it opens, so tests and
the verify skill open a copy of `ENC_ROOT`, never this folder.

## Font

`fonts/DejaVuSansMono.ttf` is DejaVu Sans Mono 2.37, `ttf/DejaVuSansMono.ttf`
from `dejavu-fonts-ttf-2.37.zip` at
https://github.com/dejavu-fonts/dejavu-fonts/releases/tag/version_2_37
(340,712 bytes, sha256 `b4a6c3e4faab8773f4ff761d56451646409f29abedd68f05d38c2df667d3c582`).
`fonts/LICENSE` is the license from the same zip, which the font's terms
require beside it. `tests/goldens.rs` draws every golden's text with it, so
the goldens don't depend on the fonts a machine has.

## Logbook

`logbook/tracks/` is synthetic: one day, 2026-09-21, sailed out of Berkeley
Marina and back past Alcatraz, written in omalogbook's GPX and file naming.
Two passages with a fix every 30 seconds (21 and 25 fixes) and one hole
between them, from 18:53:30 to 21:02:10 UTC. The points lie on straight
legs between the marina, the breakwater light and a mark east of Alcatraz.
The verify skill's trips feature drives it.
