# Test charts

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
