# Omahelm

The chartplotter for [Omahoy](https://github.com/shieldsworks/omahoy).

Omahelm draws NOAA's electronic navigational charts in your Omarchy theme:
soundings in feet, depth contours and shading, buoys and beacons labeled
the way a paper chart labels them (`G "3"`, `Fl G 4s 4M`), lights with
their sectors, rocks, wrecks and isolated dangers, traffic lanes,
anchorages and restricted areas. Your boat, its track and the AIS traffic
come from [omakeel](https://github.com/shieldsworks/omakeel).

It reads the charts itself. The S-57 reader, the chart updates, the
symbols and the renderer are all written from scratch in Rust, with no
map library or GDAL in between. The reader is checked against GDAL on
every California cell NOAA publishes: the same features and the same
vertices, updates applied.

**Status: v0.1.** It plots charts and your boat. It doesn't do routes yet.

## Not for navigation

Omahelm is not a primary means of navigation, and NOAA's charts in it are
not certified for navigation. Carry the official charts and a backup.

## Install

You need Rust (`mise install` in this checkout gets the right one) and the
Omarchy shell.

```sh
git clone https://github.com/shieldsworks/omahelm ~/.local/share/omahelm/src
cd ~/.local/share/omahelm/src
cargo build --release --locked
bash scripts/link-plugin.sh
```

`link-plugin.sh` adds Omahelm to the Omarchy shell as the plugin
`org.omahoy.helm`. Open it with `omarchy shell shell toggle org.omahoy.helm "{}"`,
or bind that to a key. To run it on its own instead: `./run.sh`.

## Charts

NOAA's charts are free. Download your waters by state:

```sh
target/release/omahelm fetch CA          # California: about 55 MB
target/release/omahelm fetch --list      # every region
target/release/omahelm fetch US5OAKFI    # or a single cell
```

Run `fetch` again to update: NOAA publishes corrections weekly. If you
already have ENCs, `omahelm import FILE.zip` or `omahelm import DIR`.
Charts go in `~/.local/share/omahelm/charts`. Cells NOAA has canceled are
skipped; `omahelm index` lists them.

## Use

| Key | |
|---|---|
| `h` `j` `k` `l`, arrows | Pan |
| `+` `-`, scroll | Zoom |
| `f` | Follow the boat, or stop (it opens following; panning stops it) |
| `c` | Center on the boat once |
| Click | What's charted here, and the wind there with the barbs on |
| `i` | What's charted at the center |
| `w`, right-click | Set a waypoint: bearing, range and ETA from the boat |
| `W` | Clear the waypoint |
| `b` | Wind barbs, from [omawind](https://github.com/shieldsworks/omawind): the forecast, and the wind NOAA's stations measured |
| `[` `]` | The wind an hour earlier, later |
| `t` | The time bar: the forecast at the boat, hour by hour |
| Space | Play the wind hour by hour |
| `n` | Night Watch: red on black, to keep your night vision |
| `?` | Keys |
| `Esc` | Close a card |
| `q` | Close |

The status bar shows the fix, speed and course over ground, where the
pointer is and how far and which way it lies from the boat, and the scale.

With the barbs on, the wind measured at NOAA's stations shows over the
forecast: the buoys offshore, and the piers and tide gauges around the Bay.
Each is a barb in the theme's accent color on a dot, with its knots on the
far side: `7g11` is 7 knots gusting 11. Point at one for its name and how
old the report is. They're for now only, so `]` to a forecast hour hides
them.

Click the chart with the barbs on and the card leads with the wind there:
the forecast for the hour on show, worked out for that very spot rather than
the nearest barb, and, for now, the nearest station within 10 nm with how
far off it is and how old its report is.

`t`, or the TIME chip beside the WIND label, opens a time bar along the
bottom, from now to the forecast's last hour; it stays closed until you
open it, and remembers. `[` `]` and space step the hours without it. Above its hour ticks is the forecast wind at the boat:
speed shaded, gusts dashed, so you can see when the breeze builds. Drag or
click to an hour, or scroll over it, and the barbs follow; the readout says
the hour and the wind at the boat then, like `Tue 14:00  250°T 12G16 kn`.
Space, or the play button, steps through the hours. Hours already fetched
are kept, so going back over them is instant.

## Settings

`~/.config/omahelm/config.toml`, all optional:

```toml
units = "feet"          # feet, meters or fathoms
safety_depth = 10       # soundings this shallow or less are drawn bold
shallow_contour = 6
safety_contour = 12     # water shallower than this is shaded as unsafe
deep_contour = 30
palette = "theme"       # theme, or paper for the colors of a paper chart
```

Depths are in `units`. Omahelm uses the shallowest contour the chart has at
or below your safety contour, the way S-52 does.

The colors follow the Omarchy theme and change with it. Colors that mean
something at sea don't: a green buoy stays green even if your theme's
"green" is amber.

Night Watch (`n`, or the NIGHT button) turns this window red on black, chart
and all, whatever the theme. With every color a red, marks are told apart
by brightness, shape and label: red buoys are bright, green ones dark,
yellow and white ones pale, and green cans stay square and red nuns
pointed. It's off each time Omahelm starts, and it changes only this
window, so Omalookout beside it can stay as it is.

## How it works

`omahelm serve` is the chart engine. It indexes the charts, draws 256-pixel
tiles on demand into `~/.cache/omahelm/tiles`, and answers what's charted
at a point. The window is a thin Quickshell client that places the tiles
and draws the boat and AIS traffic on top. They talk over a Unix socket,
described in [docs/protocol.md](docs/protocol.md).

The window starts the engine when it isn't running. `omahelm render`
draws a view to a PNG without either.

## Data

Charts: NOAA Office of Coast Survey electronic navigational charts, public
domain. The S-57 object catalogue tables are generated from GDAL's copies
(MIT).

## License

MIT
