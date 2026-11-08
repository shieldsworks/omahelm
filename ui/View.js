.pragma library
// Where the window first looks. The order is where it was left, then home,
// then the charts, then the Bay. Null means no answer yet: the engine's
// state hasn't arrived and the wait for it isn't over.

var HOME_ZOOM = 13;
var DEFAULT = {lat: 37.8663, lon: -122.3148, zoom: 13};

function isNumber(v) { return typeof v === "number" && isFinite(v); }

function firstView(saved, home, extent, waited) {
    if (saved && isNumber(saved.lat) && isNumber(saved.lon) && isNumber(saved.zoom))
        return {from: "saved", lat: saved.lat, lon: saved.lon, zoom: saved.zoom};
    if (home && isNumber(home.lat) && isNumber(home.lon))
        return {from: "home", lat: home.lat, lon: home.lon, zoom: HOME_ZOOM};
    if (extent)
        return {from: "extent", west: extent.west, south: extent.south,
                east: extent.east, north: extent.north};
    if (waited)
        return {from: "default", lat: DEFAULT.lat, lon: DEFAULT.lon, zoom: DEFAULT.zoom};
    return null;
}
