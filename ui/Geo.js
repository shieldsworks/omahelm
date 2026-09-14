.pragma library
// Web Mercator and the chart notation the window writes: positions in
// degrees and decimal minutes, bearings in degrees true, ranges in
// nautical miles.

var MAX_LAT = 85.05112878;
var EQUATOR_M = 40075016.686;
var EARTH_NM = 3440.065;

function mercX(lon) { return (lon + 180) / 360; }
function mercY(lat) {
    var phi = Math.max(-MAX_LAT, Math.min(MAX_LAT, lat)) * Math.PI / 180;
    return (1 - Math.asinh(Math.tan(phi)) / Math.PI) / 2;
}
function lon(x) { return x * 360 - 180; }
function lat(y) { return Math.atan(Math.sinh(Math.PI * (1 - 2 * y))) * 180 / Math.PI; }

// Ground metres per logical pixel at a zoom level and latitude.
function metresPerPixel(zoom, latDeg) {
    return EQUATOR_M * Math.cos(latDeg * Math.PI / 180) / (256 * Math.pow(2, zoom));
}

function rangeNm(lat1, lon1, lat2, lon2) {
    var r = Math.PI / 180;
    var dp = (lat2 - lat1) * r, dl = (lon2 - lon1) * r;
    var a = Math.pow(Math.sin(dp / 2), 2)
        + Math.cos(lat1 * r) * Math.cos(lat2 * r) * Math.pow(Math.sin(dl / 2), 2);
    return 2 * EARTH_NM * Math.asin(Math.min(1, Math.sqrt(a)));
}

// Initial great-circle bearing, degrees true.
function bearing(lat1, lon1, lat2, lon2) {
    var r = Math.PI / 180;
    var p1 = lat1 * r, p2 = lat2 * r, dl = (lon2 - lon1) * r;
    var y = Math.sin(dl) * Math.cos(p2);
    var x = Math.cos(p1) * Math.sin(p2) - Math.sin(p1) * Math.cos(p2) * Math.cos(dl);
    return (Math.atan2(y, x) / r + 360) % 360;
}

function destination(latDeg, lonDeg, bearingDeg, nm) {
    var r = Math.PI / 180, d = nm / EARTH_NM, p1 = latDeg * r, t = bearingDeg * r;
    var p2 = Math.asin(Math.sin(p1) * Math.cos(d) + Math.cos(p1) * Math.sin(d) * Math.cos(t));
    var l2 = lonDeg * r + Math.atan2(Math.sin(t) * Math.sin(d) * Math.cos(p1),
                                     Math.cos(d) - Math.sin(p1) * Math.sin(p2));
    return { lat: p2 / r, lon: l2 / r };
}

// 37°51.978′N
function dm(value, pos, neg) {
    var a = Math.abs(value), d = Math.floor(a), m = (a - d) * 60;
    if (m >= 59.9995) { d += 1; m = 0; }
    var mins = m.toFixed(3);
    if (m < 10) mins = "0" + mins;
    return d + "°" + mins + "′" + (value < 0 ? neg : pos);
}
function position(latDeg, lonDeg) { return dm(latDeg, "N", "S") + " " + dm(lonDeg, "E", "W"); }

// 042°
function degrees(deg) {
    var d = Math.round(deg) % 360;
    return (d < 10 ? "00" : d < 100 ? "0" : "") + d + "°";
}

function grouped(n) { return String(Math.round(n)).replace(/\B(?=(\d{3})+(?!\d))/g, ","); }

// The chart scale on show, 1:27,000: two significant figures, as a
// paper chart would print it.
function scaleText(zoom, latDeg) {
    var s = metresPerPixel(zoom, latDeg) / 0.00028;
    var mag = Math.pow(10, Math.max(0, Math.floor(Math.log(s) / Math.LN10) - 1));
    return "1:" + grouped(Math.round(s / mag) * mag);
}

// The longest round length in nautical miles that fits.
function niceNm(maxNm) {
    var steps = [0.01, 0.02, 0.05, 0.1, 0.2, 0.25, 0.5, 1, 2, 5, 10, 20, 50, 100, 200, 500];
    var best = steps[0];
    for (var i = 0; i < steps.length; i++) if (steps[i] <= maxNm) best = steps[i];
    return best;
}

function nmText(nm) {
    var digits = nm < 1 ? 2 : nm < 10 ? 1 : 0;
    return nm.toFixed(digits) + " nm";
}

// Time to cover a range at a speed, or "" when not making way.
function eta(nm, knots) {
    if (!(knots > 0.3)) return "";
    var minutes = nm / knots * 60;
    if (minutes < 60) return Math.max(1, Math.round(minutes)) + " min";
    var h = Math.floor(minutes / 60);
    if (h >= 48) return Math.round(h / 24) + " d";
    return h + " h " + Math.round(minutes % 60) + " min";
}
