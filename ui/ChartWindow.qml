import QtQuick
import Quickshell
import Quickshell.Io
import "Geo.js" as Geo

// The chartplotter window: the chart, the boat and AIS from omakeel, wind
// barbs from omawind, a status bar, and the keys. Run standalone
// (ui/shell.qml) it owns its process; as the shell's panel the shell opens
// and hides it.
Item {
    id: app

    // Set by the Omarchy shell when loaded as a panel.
    property var shell: null
    property var manifest: null
    property bool standalone: true
    property bool opened: standalone

    function open(payload) {
        opened = true;
        Qt.callLater(() => surface.forceActiveFocus());
    }
    function close() {
        save();
        opened = false;
    }
    function dismiss() {
        save();
        if (standalone) Qt.quit();
        else if (shell) shell.hide("org.omahoy.helm");
        else opened = false;
    }

    property Theme theme: Theme {}
    property Helm helm: Helm {}
    property Keel keel: Keel {}
    property Wind wind: Wind { wanted: app.windOn }
    property Stream stream: Stream { wanted: app.streamOn }

    // Quickshell keeps a process alive after its last window closes.
    Connections {
        target: Quickshell
        function onLastWindowClosed() { if (app.standalone) Qt.quit(); }
    }

    // ------------------------------------------------------------ the boat

    readonly property var fix: keel.fix
    readonly property bool hasPosition: !!fix && typeof fix.lat === "number" && typeof fix.lon === "number"
    readonly property bool fixOk: hasPosition && fix.status === "ok"
    // The window opens following the boat; panning by hand stops it.
    property bool follow: true
    // The first follow zooms in from a view too wide to steer by.
    property bool followedOnce: false
    // This session's track, in Mercator units, oldest first.
    property var track: []
    property var waypoint: null

    onFixChanged: {
        // A stale position still places the boat: where it was last seen.
        if (follow && hasPosition && placed) followBoat();
        if (!fixOk) return;
        var p = {x: Geo.mercX(fix.lon), y: Geo.mercY(fix.lat), lat: fix.lat, lon: fix.lon};
        var last = track.length ? track[track.length - 1] : null;
        // A point every 5 m of way made, at most 2000 of them.
        if (!last || Geo.rangeNm(last.lat, last.lon, p.lat, p.lon) * 1852 > 5) {
            var next = track.length >= 2000 ? track.slice(track.length - 1999) : track.slice();
            next.push(p);
            track = next;
        }
    }

    function followBoat() {
        if (!followedOnce && map.zoom < 11) map.zoom = 14;
        followedOnce = true;
        map.lookAt(fix.lat, fix.lon);
    }

    function toggleFollow() {
        if (follow) { follow = false; return; }
        if (!hasPosition) { toast("No position from the GPS yet"); return; }
        follow = true;
        map.lookAt(fix.lat, fix.lon);
    }
    // Night Watch for this window only; the chart goes red with it once the
    // engine has drawn it.
    function toggleNight() {
        theme.night = !theme.night;
        nightWarned = false;
        warnNight();
    }
    // Said once when night meets an engine that draws only the theme's
    // tiles, whether night or the engine came first.
    property bool nightWarned: false
    function warnNight() {
        var old = theme.night && !!helm.state && !helm.state.tiles.night;
        if (old && !nightWarned) toast("Red chart needs a newer engine: quit omahelm serve and reopen");
        nightWarned = old;
    }
    function centerOnBoat() {
        if (!hasPosition) { toast(keel.incompatible ? "omakeel speaks a newer protocol: update omahelm" : keel.connected ? "No position from the GPS yet" : "No GPS: omakeel isn't running"); return; }
        map.lookAt(fix.lat, fix.lon);
    }

    // ---------------------------------------------------------- the engine

    property int queryId: 0
    property var result: null       // the last `features`, or null while asking
    property bool cardOpen: false
    property point cardAt: Qt.point(0, 0)

    function query(lat, lon, x, y) {
        queryId += 1;
        result = null;
        cardAt = Qt.point(x, y);
        cardOpen = true;
        map.mark = {lat: lat, lon: lon};
        askPoint(true);
        if (!helm.connected) { result = {features: [], lost: true}; return; }
        helm.send({type: "query", id: queryId, lat: lat, lon: lon, zoom: Math.round(map.zoom)});
    }
    function closeCard() {
        cardOpen = false;
        map.mark = null;
        askPoint(true);
    }
    function setWaypoint(lat, lon) {
        waypoint = {lat: lat, lon: lon};
        saveSoon.restart();
    }

    Connections {
        target: app.helm
        function onTile(m) { map.tileArrived(m); }
        function onFeatures(m) { if (m.id === app.queryId) app.result = m; }
        function onRejected(message) { app.toast(message); }
        function onStateChanged() { app.placeCamera(); app.warnNight(); }
        // A question the engine can no longer answer.
        function onConnectedChanged() { if (!app.helm.connected && app.cardOpen && app.result === null) app.result = {features: [], lost: true}; }
    }

    // ---------------------------------------------------------- the wind

    // omawind's wind barbs, now or at a whole hour ahead: b, [ and ], the
    // scrubber along the bottom, and space to play the hours.
    property bool windOn: false
    // The time bar along the bottom: closed until t or the TIME chip opens
    // it, and remembered. [ ], space and the WIND label work without it.
    property bool timeBar: false
    function toggleTimeBar() {
        // With neither layer on there are no hours to show, so t turns the
        // wind on, as it always did. With either one on it is just a
        // toggle, and the bar scrubs whichever are on.
        if (!windOn && !streamOn) {
            toggleWind();
            timeBar = true;
        } else timeBar = !timeBar;
    }
    // The hour chosen, as UTC milliseconds, or 0 for now. It's absolute,
    // so the barbs stay right as the clock turns over.
    property real windAt: 0
    onWindAtChanged: if (cardOpen) askPoint(true)
    property int windId: 0
    property var windField: null    // the last `field` asked for
    property string windError: ""
    property bool windPlaying: false
    // Space pressed before the forecast was in: play once it is.
    property bool windPlayPending: false
    // Fields already fetched, by hour, for the view and run in windView, so
    // scrubbing back over an hour is instant; a new view or run starts
    // afresh. windAsked maps a request's id to the hour it asked for.
    property var windCache: ({})
    property var windAsked: ({})
    property string windView: ""
    property int windAhead: 0
    // Read by the bindings below that follow the clock.
    property real minute: Date.now()
    Timer { interval: 30000; repeat: true; running: app.windOn; onTriggered: app.minute = Date.now() }

    function hourNow() { return Math.floor(Date.now() / 3600e3) * 3600e3; }
    // The forecast's last hour. No run goes past 48 hours, so a later end
    // is a broken one and mustn't make the scrubber endless.
    function windLast() {
        var f = wind.forecast;
        var t = f && typeof f.last === "string" ? Date.parse(f.last) : NaN;
        return isNaN(t) ? 0 : Math.min(t, hourNow() + 48 * 3600e3);
    }
    // Hours from the hour under way to the one chosen.
    readonly property int windHours: {
        void app.minute;
        return windAt > 0 ? Math.round((windAt - hourNow()) / 3600e3) : 0;
    }
    // A tide can be predicted for ever, so the bar has to stop somewhere.
    // A day shows two full cycles of the stream, which is what it takes to
    // pick the slack you want.
    readonly property int tideSpan: 24
    // How many hours the time bar covers: as far as the layers that are on
    // can speak for. Past the forecast's end the barbs go, and the arrows
    // keep going, which is honest — the tide is still known there.
    readonly property int scrubSpan: Math.max(windOn ? windSpan : 0, streamOn ? tideSpan : 0)
    // The hour the layers are showing. One clock for both of them: a drag
    // moves the wind at the Gate and the stream under it together.
    readonly property int scrubHours: windHours

    // Hours from the one under way to the forecast's last: how far the
    // wind alone can be scrubbed. None without a forecast that covers now.
    readonly property int windSpan: {
        void app.minute;
        var f = wind.forecast, last = windLast();
        if (!f || f.status === "none" || f.status === "expired") return 0;
        return last > hourNow() ? Math.round((last - hourNow()) / 3600e3) : 0;
    }
    // The forecast at the boat hour by hour, for the scrubber's graph and
    // readout: only hours with a time and numbers that can be drawn.
    readonly property var windOutlook: {
        var s = wind.state;
        if (!s || !Array.isArray(s.outlook)) return [];
        function num(v, lo, hi) { return typeof v === "number" && isFinite(v) && v >= lo && v <= hi; }
        return s.outlook.filter(h => h !== null && typeof h === "object" && typeof h.time === "string"
            && !isNaN(Date.parse(h.time)) && num(h.speedKn, 0, 250) && num(h.dirDeg, 0, 360)
            && (h.gustKn === undefined || num(h.gustKn, 0, 300)));
    }
    // The scrubber's readout: the hour chosen and the wind forecast at the
    // boat then, as the bar puts it.
    readonly property string scrubText: {
        void app.minute;
        var when = windAt > 0 ? Qt.formatDateTime(new Date(windAt), "ddd HH:mm") : "Now";
        // With only the stream on, the bar is the tide's, so it reads the
        // stream. With the wind on it stays the wind's, as it was.
        if (streamOn && !windOn) {
            var kn = streamKnots(windAt > 0 ? windAt : hourNow());
            var words = streamWords(kn);
            return words ? when + "   " + words : when;
        }
        // Now is the wind at the boat this minute, as the barbs are; an
        // hour ahead is that hour's forecast.
        var here = wind.state ? wind.state.here : null;
        var h = windAt > 0 ? windOutlook.find(o => Date.parse(o.time) === windAt)
            : here && typeof here.speedKn === "number" && isFinite(here.speedKn)
              && typeof here.dirDeg === "number" && isFinite(here.dirDeg) ? here : null;
        return h ? when + "   " + windWords(h) : when;
    }
    readonly property string scrubWhere: {
        if (streamOn && !windOn)
            return streamCurve && typeof streamCurve.name === "string"
                ? "stream at " + streamCurve.name : "the stream mid-chart";
        return wind.state && wind.state.here && wind.state.here.at === "home"
            ? "forecast at home" : "forecast at the boat";
    }

    // Barbs for another hour or run mustn't stay up under a new label: an
    // answer still on its way is ignored, and the barbs go until the next.
    function forgetWind() {
        windId += 1;
        windField = null;
    }
    function toggleWind() {
        windOn = !windOn;
        windError = "";
        forgetWind();
        if (!windOn) {
            windAt = 0;
            windPlaying = false;
            windPlayPending = false;
        } else windSettle.restart();
        askPoint(true);
    }
    // An hour the clock has reached is now; one past the bar's end is its
    // last.
    function clampWind(at) {
        var now = hourNow(), last = now + scrubSpan * 3600e3;
        if (at > 0 && at > last) at = last;
        return at > now ? at : 0;
    }
    function stepWind(d) {
        windPlaying = false;
        windPlayPending = false;
        // With a layer already on, the hour is that layer's; with none on,
        // the wind is what stepping through hours used to mean.
        if (!windOn && !streamOn) windOn = true;
        var next = clampWind((windAt > 0 ? windAt : hourNow()) + d * 3600e3);
        if (next !== windAt) {
            windAt = next;
            forgetWind();
        }
        windSettle.restart();
    }
    // The hour `i` on from the one under way, 0 being now: where the
    // scrubber and play land. An hour already fetched shows at once.
    // An hour picked by hand: it cancels a play still waiting to start.
    function scrubTo(i) {
        windPlayPending = false;
        i = Math.max(0, Math.min(scrubSpan, Math.round(i)));
        scrubAt(i === 0 ? 0 : hourNow() + i * 3600e3);
    }
    // The same, by the hour's time, 0 being now.
    function scrubAt(at) {
        if (!windOn && !streamOn) windOn = true;
        var next = clampWind(at);
        if (next === windAt) return;
        windAt = next;
        forgetWind();
        requestWind();
    }
    // The hour after the one on show, by its time, so play can't skip one
    // as the clock turns over; 0 past the forecast's end.
    function nextHour() {
        var at = (windAt > 0 ? windAt : hourNow()) + 3600e3;
        var last = hourNow() + scrubSpan * 3600e3;
        return scrubSpan > 0 && at <= last ? at : 0;
    }
    // Space: the hours one after another, from now if at the end.
    function togglePlay() {
        if (windPlayPending) {
            windPlayPending = false;
            return;
        }
        // The layer just turned on, or omawind hasn't answered yet: play
        // once the forecast is in, or say then that there's nothing to.
        // The stream can play at once; the wind has to wait for omawind.
        if (!streamOn && (!windOn || !wind.state)) {
            windOn = true;
            windPlayPending = true;
            return;
        }
        if (windPlaying) {
            windPlaying = false;
            return;
        }
        if (scrubSpan === 0) {
            toast("No forecast hours ahead to play");
            return;
        }
        if (!nextHour()) scrubAt(0);
        windPlaying = true;
        prefetchAt(nextHour());
    }
    Timer {
        interval: 900
        repeat: true
        running: app.windPlaying && (app.windOn || app.streamOn)
        onTriggered: {
            var at = app.nextHour();
            if (!at) {
                app.windPlaying = false;
                return;
            }
            app.scrubAt(at);
            app.prefetchAt(app.nextHour());
        }
    }
    // The view and a margin around it, thinned to a barb every 70 pixels
    // or so; null when there's no view.
    function windArea() {
        if (map.width <= 0 || map.height <= 0) return null;
        var w = map.width / map.world, h = map.height / map.world;
        function clampLat(v) { return Math.max(-85, Math.min(85, v)); }
        var north = clampLat(Geo.lat(map.cy - h * 0.6)), south = clampLat(Geo.lat(map.cy + h * 0.6));
        var west = Math.max(-180, Geo.lon(map.cx - w * 0.6)), east = Math.min(180, Geo.lon(map.cx + w * 0.6));
        if (!(south < north && west < east)) return null;
        return {south: south, west: west, north: north, east: east,
                max: Math.max(1, Math.min(2000, Math.floor(map.width * map.height / 4900)))};
    }
    // Hours are kept for one view, one run and one forecast region:
    // anything else starts afresh.
    function windCacheFor(area) {
        var f = wind.forecast;
        var view = [area.south, area.west, area.north, area.east].map(v => v.toFixed(5)).join(",")
            + "," + area.max + "|" + (f && typeof f.run === "string" ? f.run : "")
            + "|" + (f && f.region ? JSON.stringify(f.region) : "");
        if (view !== windView || Object.keys(windCache).length > 120) {
            windView = view;
            windCache = ({});
            windAsked = ({});
        }
    }
    // Asks for one hour's field, or now's with `at` 0, under `id`. An
    // hour's answer is kept; now's isn't, since now moves.
    function askWind(area, at, id) {
        var request = {type: "field", id: id, south: area.south, west: area.west, north: area.north,
                       east: area.east, max: area.max};
        if (at > 0) {
            request.time = utc(at);
            windAsked[id] = String(at);
        }
        wind.send(request);
    }
    function requestWind() {
        if (!windOn || !wind.connected) return;
        var f = wind.forecast;
        // Nothing to ask: no forecast, or one that has run out.
        if (!f || f.status === "none" || f.status === "expired") {
            forgetWind();
            return;
        }
        var at = clampWind(windAt);
        if (at !== windAt) {
            windAt = at;
            forgetWind();
        }
        // The bar can run past the forecast when the stream is on. There
        // are no barbs out there; omawind would only answer with an error.
        var last = windLast();
        if (windAt > 0 && (!last || windAt > last)) {
            forgetWind();
            return;
        }
        var area = windArea();
        if (!area) return;
        windCacheFor(area);
        var cached = windAt > 0 ? windCache[String(windAt)] : undefined;
        if (cached) {
            // An answer still on its way is for another hour now.
            windId += 1;
            windField = cached;
            windError = "";
            return;
        }
        askWind(area, windAt, ++windId);
    }
    // The hour at `at` fetched ahead while playing, so it's there when
    // play is.
    function prefetchAt(at) {
        if (!windOn || !wind.connected || !at || at <= hourNow() || at > windLast()) return;
        var area = windArea();
        if (!area) return;
        windCacheFor(area);
        var key = String(at);
        if (windCache[key] || Object.keys(windAsked).some(id => windAsked[id] === key)) return;
        askWind(area, Number(key), "ahead" + (++windAhead));
    }
    // An hour as omawind takes it, in UTC.
    function utc(at) { return new Date(at).toISOString().slice(0, 19) + "Z"; }
    Timer { id: windSettle; interval: 150; onTriggered: app.requestWind() }
    Connections {
        target: app.wind
        function onField(m) {
            // Kept under the hour it answers, if it's for this view and run.
            var key = app.windAsked[m.id];
            if (key !== undefined) {
                delete app.windAsked[m.id];
                if (typeof m.time === "string" && Date.parse(m.time) === Number(key)) app.windCache[key] = m;
            }
            if (m.id !== app.windId) return;
            app.windField = m;
            app.windError = "";
        }
        function onPoint(m) {
            if (m.id !== "point" + app.pointId) return;
            app.cardWind = m;
            app.cardWindError = "";
        }
        // Said once, not at every minute's new request.
        function onRejected(m) {
            // The card's question: said on the card.
            if (m.id === "point" + app.pointId) {
                app.cardWind = null;
                app.cardWindError = String(m.message || "");
                return;
            }
            delete app.windAsked[m.id];
            if (m.id !== app.windId) return;
            app.windField = null;
            var said = String(m.message || "");
            if (said !== app.windError) app.toast(said);
            app.windError = said;
        }
        // A new minute or a new run: ask again. No forecast: no barbs.
        function onStateChanged() {
            var f = app.wind.forecast;
            if (!f || f.status === "none") app.forgetWind();
            else if (app.windOn) windSettle.restart();
            if (app.windPlayPending && app.windOn && app.wind.state) {
                app.windPlayPending = false;
                app.togglePlay();
            }
            if (app.cardOpen && app.windOn) app.refreshPoint();
        }
    }
    Connections {
        target: map
        function onCxChanged() { if (app.windOn) windSettle.restart(); }
        function onCyChanged() { if (app.windOn) windSettle.restart(); }
        function onZoomChanged() { if (app.windOn) windSettle.restart(); }
        function onWidthChanged() { if (app.windOn) windSettle.restart(); }
        function onHeightChanged() { if (app.windOn) windSettle.restart(); }
    }
    // The wind the stations measured: with the barbs, and only for now.
    readonly property var windStations: windOn && windAt === 0 && wind.connected && !wind.incompatible
        && wind.stations ? wind.stations.stations : []
    readonly property string windText: {
        void app.minute;
        if (wind.incompatible) return "WIND  omawind speaks a newer protocol: update omahelm";
        if (!wind.connected) return "WIND  omawind isn't running";
        var s = wind.stations;
        var measured = windAt > 0 || !s ? ""
            : windStations.length ? "   " + windStations.length + " stations"
            : s.status === "error" ? "   stations unreachable" : "";
        var f = wind.forecast;
        if (!f || f.status === "none") return "WIND  no forecast yet" + measured;
        // The run the barbs on show came from, once they're in.
        var run = windField && typeof windField.run === "string" ? windField.run : f.run;
        var runText = "HRRR " + Qt.formatDateTime(new Date(run), "HH:mm") + " run";
        if (f.status === "expired") return "WIND  the " + runText + " has run out" + measured;
        var when = windAt > 0 ? "+" + windHours + " h  " + Qt.formatDateTime(new Date(windAt), "ddd HH:mm") : "now";
        return "WIND " + when + "   " + runText + (f.status === "old" ? ", old" : "") + measured;
    }
    // Wind as the bar puts it, `110°T 3G4 kn`, or calm with no direction.
    function windWords(h) {
        if (typeof h.dirDeg !== "number") return "calm";
        var kn = Math.round(h.speedKn);
        var gust = typeof h.gustKn === "number" && isFinite(h.gustKn) && Math.round(h.gustKn) > kn
            ? "G" + Math.round(h.gustKn) : "";
        return Geo.degrees(h.dirDeg) + "T " + kn + gust + " kn";
    }
    // When a station's report was taken: `at 17:00, 12 min ago`.
    function reportAge(s) {
        var t = Date.parse(s.time);
        if (isNaN(t)) return "";
        var ago = Math.max(0, Math.round((Date.now() - t) / 60000));
        return "at " + Qt.formatDateTime(new Date(t), "HH:mm") + ", " + ago + " min ago";
    }
    // A station under the pointer: its name, its wind, and when.
    // -------------------------------------------------------- the stream

    // omatide's tidal stream at NOAA's current stations in view: s.
    //
    // Simpler than the wind, and for a reason. omawind predicts a grid,
    // so omahelm caches fields per hour and fetches the next one ahead
    // while playing. A tide is arithmetic: omatide answers in a
    // millisecond for any moment, however far off, so there is nothing
    // worth caching and nothing to fetch ahead.
    property bool streamOn: false
    property var streamArrows: []
    property string streamError: ""

    function toggleStream() {
        streamOn = !streamOn;
        streamError = "";
        if (!streamOn) {
            streamArrows = [];
            streamCurve = null;
        } else streamSettle.restart();
    }

    // The arrows follow the hour the time bar is showing, so one drag
    // moves the wind and the stream together. 0 is now.
    readonly property real streamAt: windAt

    // The chart's corners, with a margin so an arrow just off the edge is
    // there when a pan brings it in.
    function streamArea() {
        var pad = 0.25;
        var sw = {lat: Geo.lat(map.cy + (map.height / 2) / map.world),
                  lon: Geo.lon(map.cx - (map.width / 2) / map.world)};
        var ne = {lat: Geo.lat(map.cy - (map.height / 2) / map.world),
                  lon: Geo.lon(map.cx + (map.width / 2) / map.world)};
        var dLat = (ne.lat - sw.lat) * pad, dLon = (ne.lon - sw.lon) * pad;
        return {south: Math.max(-85, sw.lat - dLat), north: Math.min(85, ne.lat + dLat),
                west: Math.max(-180, sw.lon - dLon), east: Math.min(180, ne.lon + dLon)};
    }

    function requestStream() {
        if (!streamOn || !stream.connected) return;
        var area = streamArea();
        // A view wrapped round the date line would ask for the whole
        // world; omatide would answer, but nothing here can draw it.
        if (area.east <= area.west) return;
        var request = {type: "streams", id: ++streamId,
                       south: area.south, west: area.west,
                       north: area.north, east: area.east};
        if (streamAt > 0) request.time = new Date(streamAt).toISOString().replace(/\.\d+Z$/, "Z");
        stream.send(request);
    }
    property int streamId: 0

    Timer {
        id: streamSettle
        interval: 150
        onTriggered: {
            app.requestStream();
            app.requestStreamCurve();
        }
    }
    onStreamAtChanged: if (streamOn) streamSettle.restart()

    Connections {
        target: app.stream
        function onStreams(m) {
            // Only the newest request's answer; an older one arriving late
            // would put the chart back an hour.
            if (m.id !== app.streamId) return;
            app.streamArrows = m.streams;
            app.streamError = "";
        }
        function onRejected(m) {
            if (m.id !== app.streamId) return;
            app.streamArrows = [];
            app.streamError = typeof m.message === "string" ? m.message : "omatide refused the request";
        }
        function onConnectedChanged() {
            if (app.stream.connected) streamSettle.restart();
            else {
                app.streamArrows = [];
                app.streamCurve = null;
            }
        }
    }

    Connections {
        target: map
        function onCxChanged() { if (app.streamOn) streamSettle.restart(); }
        function onCyChanged() { if (app.streamOn) streamSettle.restart(); }
        function onZoomChanged() { if (app.streamOn) streamSettle.restart(); }
        function onWidthChanged() { if (app.streamOn) streamSettle.restart(); }
        function onHeightChanged() { if (app.streamOn) streamSettle.restart(); }
    }

    // The stream is predicted, not observed, so it goes stale by the
    // minute like the wind does: re-ask every ten while it's on and live.
    Timer {
        interval: 600000
        repeat: true
        running: app.streamOn && app.streamAt === 0
        onTriggered: app.requestStream()
    }

    // The stream through the hours at the middle of the chart, for the
    // time bar's graph and its readout. omatide resolves the position to
    // its nearest station, so the bar speaks for where you are looking,
    // not for where the boat happens to be.
    property var streamCurve: null
    property int streamCurveId: 0

    function requestStreamCurve() {
        if (!streamOn || !stream.connected || scrubSpan <= 0) return;
        stream.send({type: "curve", id: "curve" + (++streamCurveId), kind: "current",
                     lat: map.centerLat, lon: map.centerLon,
                     time: new Date(hourNow()).toISOString().replace(/\.\d+Z$/, "Z"),
                     hours: scrubSpan, stepSeconds: 1800});
    }
    onScrubSpanChanged: if (streamOn) streamSettle.restart()

    Connections {
        target: app.stream
        function onCurve(m) {
            if (m.id !== "curve" + app.streamCurveId) return;
            app.streamCurve = m.values.length ? m : null;
        }
    }

    // The stream on the curve at a moment, in knots, positive on the
    // flood. NaN outside it. Not `streamAt`: that property is a time, and
    // a property shadows a function of the same name.
    function streamKnots(when) {
        var c = streamCurve;
        if (!c) return NaN;
        var start = Date.parse(c.start), step = c.stepSeconds * 1000;
        var i = Math.round((when - start) / step);
        return i >= 0 && i < c.values.length ? c.values[i] : NaN;
    }
    function streamWords(kn) {
        if (isNaN(kn)) return "";
        if (Math.abs(kn) < 0.05) return "slack";
        return Math.abs(kn).toFixed(1) + " kn " + (kn > 0 ? "flood" : "ebb");
    }

    readonly property string streamText: {
        if (!streamOn) return "";
        if (stream.incompatible) return "STREAM  omatide speaks a newer protocol: update omahelm";
        if (!stream.connected) return "STREAM  omatide isn't running";
        if (stream.stations === 0) return "STREAM  no stations: run `omatide fetch`";
        if (streamError !== "") return "STREAM  " + streamError;
        var when = streamAt > 0
            ? "+" + scrubHours + " h  " + Qt.formatDateTime(new Date(streamAt), "ddd HH:mm") : "now";
        if (!streamArrows.length) return "STREAM " + when + "   none in view";
        // Say when arrows were left out, so a thin chart isn't read as a
        // slack bay. Zooming in brings them back.
        var shown = map.shownStreams;
        var room = shown > 0 && shown < streamArrows.length
            ? "   " + shown + " of " + streamArrows.length + " stations"
            : "   " + streamArrows.length + " stations";
        return "STREAM " + when + room;
    }

    // One station's stream, for the cursor readout.
    function streamTextFor(s) {
        var name = s.name || s.station;
        if (s.way === "slack" || s.knots < 0.05) return name + "  slack";
        var set = typeof s.setDeg === "number" ? Geo.degrees(s.setDeg) + "T " : "";
        var depth = typeof s.depthM === "number" ? "  at " + s.depthM.toFixed(1) + " m" : "";
        return name + "  " + set + s.knots.toFixed(1) + " kn " + s.way + depth;
    }

    function stationText(s) {
        void app.minute;
        var age = reportAge(s);
        return (s.name || s.id) + "  " + windWords(s) + (age ? "  " + age : "");
    }

    // The wind on the card, with the barbs on: the forecast where the card
    // was asked, for the hour on show, and the nearest station's for now.
    property int pointId: 0
    property var cardWind: null     // the last `point`, or null while asking
    property string cardWindError: ""
    // What the last `point` was asked under, so a state that brings only a
    // new fix doesn't ask again.
    property string pointKey: ""
    function pointKeyNow() {
        var f = wind.forecast;
        return (f ? f.run + "|" + f.status + "|" + JSON.stringify(f.region) : "")
            + "|" + (windAt > 0 ? windAt : Math.floor(Date.now() / 60000));
    }
    // `fresh` forgets the last answer, for a new place or hour; a refresh
    // leaves it up until the next.
    function askPoint(fresh) {
        pointId += 1;
        pointKey = "";
        if (fresh) {
            cardWind = null;
            cardWindError = "";
        }
        var f = wind.forecast;
        if (!cardOpen || !map.mark || !windOn || !wind.connected
            || !f || f.status === "none" || f.status === "expired") return;
        var request = {type: "point", id: "point" + pointId, lat: map.mark.lat, lon: map.mark.lon};
        if (windAt > 0) request.time = utc(windAt);
        pointKey = pointKeyNow();
        wind.send(request);
    }
    // Asked again once the run, or for now the minute, has moved on.
    function refreshPoint() { if (pointKey !== pointKeyNow()) askPoint(false); }
    readonly property string cardForecastText: {
        if (wind.incompatible) return "omawind speaks a newer protocol: update omahelm";
        if (!wind.connected) return "omawind isn't running";
        var f = wind.forecast;
        if (!f || f.status === "none") return "No forecast yet";
        if (f.status === "expired") return "The forecast has run out";
        if (cardWindError)
            return cardWindError.startsWith("unknown type") ? "Update omawind to see it here" : cardWindError;
        if (!cardWind) return "Asking omawind…";
        if (typeof cardWind.speedKn !== "number")
            return cardWind.note ? cardWind.note.charAt(0).toUpperCase() + cardWind.note.slice(1) : "No forecast here";
        return windWords(cardWind);
    }
    // The hour it's for, and the run it came from. The answer shown is
    // always for the hour last asked, so that's the one named.
    readonly property string cardForecastWhen: {
        if (!cardWind || typeof cardWind.speedKn !== "number") return "";
        var run = Date.parse(cardWind.run);
        var when = windAt > 0 ? Qt.formatDateTime(new Date(windAt), "ddd HH:mm") : "Now";
        return when + (isNaN(run) ? "" : ", from the " + Qt.formatDateTime(new Date(run), "HH:mm") + " run");
    }
    // The station nearest the card's place, within 10 nm, for now only:
    // {station, nm}, or null.
    readonly property var cardStation: {
        var p = map.mark;
        if (!cardOpen || !p || windAt > 0) return null;
        var best = null;
        for (var i = 0; i < windStations.length; i++) {
            var s = windStations[i], nm = Geo.rangeNm(p.lat, p.lon, s.lat, s.lon);
            if (nm <= 10 && (!best || nm < best.nm)) best = {station: s, nm: nm};
        }
        return best;
    }
    // How far off it is, which way, and when it measured.
    readonly property string cardStationWhere: {
        void app.minute;
        var c = cardStation, p = map.mark;
        if (!c || !p) return "";
        var age = reportAge(c.station);
        return Geo.nmText(c.nm) + " " + Geo.degrees(Geo.bearing(p.lat, p.lon, c.station.lat, c.station.lon))
            + "T from here" + (age ? ", " + age : "");
    }

    // ---------------------------------------------------------- the view

    // $XDG_STATE_HOME/omahelm/view.json: the last camera and the waypoint.
    // Following isn't kept: the window always opens following the boat.
    readonly property string viewPath: (Quickshell.env("XDG_STATE_HOME") || Quickshell.env("HOME") + "/.local/state") + "/omahelm/view.json"
    property var saved: null
    property bool savedRead: false
    property bool placed: false

    FileView {
        path: app.viewPath
        printErrors: false
        onLoaded: {
            try { app.saved = JSON.parse(text()); } catch (e) { app.saved = null; }
            app.savedRead = true;
            app.placeCamera();
        }
        onLoadFailed: { app.savedRead = true; app.placeCamera(); }
    }

    // The first view: where we left off, else the charts, else the Bay.
    function placeCamera() {
        if (placed || !savedRead || map.width <= 0) return;
        var s = saved;
        if (s && typeof s.lat === "number" && typeof s.lon === "number" && typeof s.zoom === "number") {
            map.zoom = Math.max(map.minZoom, Math.min(map.maxZoom, s.zoom));
            map.lookAt(s.lat, s.lon);
            if (s.waypoint && typeof s.waypoint.lat === "number" && typeof s.waypoint.lon === "number")
                waypoint = {lat: s.waypoint.lat, lon: s.waypoint.lon};
            if (s.wind === true) windOn = true;
            if (s.stream === true) streamOn = true;
            if (s.timeBar === true) timeBar = true;
        } else if (helm.state && helm.state.charts && helm.state.charts.extent) {
            var e = helm.state.charts.extent;
            map.fit(e.west, e.south, e.east, e.north);
        } else if (!firstViewWait.fired) {
            return;
        } else {
            map.zoom = 13;
            map.lookAt(37.8663, -122.3148);
        }
        placed = true;
        if (follow && hasPosition) followBoat();
    }
    Timer {
        id: firstViewWait
        property bool fired: false
        interval: 2500
        running: true
        onTriggered: { fired = true; app.placeCamera(); }
    }

    function save() {
        if (!placed) return;
        var view = {lat: map.centerLat, lon: map.centerLon, zoom: Math.round(map.zoom * 100) / 100};
        if (waypoint) view.waypoint = waypoint;
        if (windOn) view.wind = true;
        if (streamOn) view.stream = true;
        if (timeBar) view.timeBar = true;
        var text = JSON.stringify(view);
        var slash = viewPath.lastIndexOf("/");
        writer.command = ["sh", "-c", 'mkdir -p -- "$1" && printf "%s\\n" "$3" > "$2" && mv -f -- "$2" "$4"',
                          "omahelm-view", viewPath.slice(0, slash), viewPath + ".tmp", text, viewPath];
        writer.running = true;
    }
    Process { id: writer; command: ["true"] }
    Timer { id: saveSoon; interval: 1500; onTriggered: app.save() }

    // ---------------------------------------------------------- messages

    property string toastText: ""
    function toast(text) {
        toastText = text;
        toastTimer.restart();
    }
    Timer { id: toastTimer; interval: 3500; onTriggered: app.toastText = "" }

    // What fills the middle of the screen when there's no chart to show.
    readonly property string notice: {
        if (helm.incompatible) return helm.error;
        if (!helm.connected) {
            if (!helm.waited) return "Starting the chart engine…";
            return "The chart engine isn't running.\nTried: " + helm.binary + " serve"
                + (helm.lastLog ? "\n\n" + helm.lastLog : "");
        }
        if (!helm.state) return "";
        var c = helm.state.charts;
        if (c.status === "indexing")
            return "Reading charts…" + (c.progress ? " " + c.progress.done + " of " + c.progress.total : "");
        if (c.status === "empty")
            return "No charts yet.\n\nDownload NOAA charts for your waters with:\n\n    omahelm fetch CA\n\nomahelm fetch --list shows every region.";
        return "";
    }
    // True when the view has no chart under it at all.
    readonly property bool offChart: {
        if (!helm.state || !helm.state.charts || !helm.state.charts.extent) return false;
        var e = helm.state.charts.extent;
        var halfW = map.width / 2 / map.world, halfH = map.height / 2 / map.world;
        return map.cx + halfW < Geo.mercX(e.west) || map.cx - halfW > Geo.mercX(e.east)
            || map.cy + halfH < Geo.mercY(e.north) || map.cy - halfH > Geo.mercY(e.south);
    }

    // ---------------------------------------------------------- status text

    readonly property string units: helm.state && helm.state.settings ? helm.state.settings.units : ""
    readonly property string gpsText: {
        if (keel.incompatible) return "GPS  omakeel speaks a newer protocol: update omahelm";
        if (!keel.connected) return "GPS  omakeel isn't running";
        if (!fix || fix.status === "none" || !hasPosition) return fix && fix.status === "nofix" ? "GPS  no fix" : "GPS  waiting";
        var head = fix.status === "ok" ? "GPS" : fix.status === "stale" ? "GPS STALE " + fix.ageSeconds + " s" : "GPS NO FIX";
        var out = head + "  " + Geo.position(fix.lat, fix.lon);
        if (fix.status === "ok" && typeof fix.sogKn === "number") out += "  " + fix.sogKn.toFixed(1) + " kn";
        if (fix.status === "ok" && typeof fix.cogDeg === "number") out += "  " + Geo.degrees(fix.cogDeg) + "T";
        return out;
    }
    readonly property color gpsColor: !keel.connected || !hasPosition ? theme.red
        : fix.status === "ok" ? theme.foreground : theme.yellow
    readonly property string cursorText: {
        if (!map.hovering) return "";
        if (map.hoverStation) return stationText(map.hoverStation);
        if (map.hoverStream) return streamTextFor(map.hoverStream);
        var out = "+ " + Geo.position(map.hoverLat, map.hoverLon);
        if (hasPosition)
            out += "  " + Geo.degrees(Geo.bearing(fix.lat, fix.lon, map.hoverLat, map.hoverLon)) + "T "
                + Geo.nmText(Geo.rangeNm(fix.lat, fix.lon, map.hoverLat, map.hoverLon));
        return out;
    }
    readonly property string waypointText: {
        if (!waypoint) return "";
        if (!hasPosition) return "WPT  " + Geo.position(waypoint.lat, waypoint.lon);
        var nm = Geo.rangeNm(fix.lat, fix.lon, waypoint.lat, waypoint.lon);
        var eta = fixOk ? Geo.eta(nm, fix.sogKn) : "";
        return "WPT  " + Geo.degrees(Geo.bearing(fix.lat, fix.lon, waypoint.lat, waypoint.lon)) + "T "
            + Geo.nmText(nm) + (eta ? "  ETA " + eta : "");
    }
    readonly property real barNm: Geo.niceNm(110 * map.metersPerPixel / 1852)
    readonly property real barPx: barNm * 1852 / map.metersPerPixel

    // ------------------------------------------------------------ keys

    function key(e) {
        var t = e.key === Qt.Key_Escape ? "Escape"
            : e.key === Qt.Key_Left ? "h" : e.key === Qt.Key_Right ? "l"
            : e.key === Qt.Key_Up ? "k" : e.key === Qt.Key_Down ? "j" : e.text;
        // A held n would flicker between palettes.
        if (t === "n" && e.isAutoRepeat) { e.accepted = true; return; }
        e.accepted = run(t);
    }

    // One key's action, by the text it types. True when it did something.
    function run(t) {
        if (sheet.visible) {
            if (t === "Escape" || t === "?" || t === "q") sheet.visible = false;
            return true;
        }
        if (t === "Escape") { if (cardOpen) closeCard(); }
        else if (t === "h") { follow = false; map.pan(-1, 0); }
        else if (t === "l") { follow = false; map.pan(1, 0); }
        else if (t === "k") { follow = false; map.pan(0, -1); }
        else if (t === "j") { follow = false; map.pan(0, 1); }
        else if (t === "+" || t === "=") map.zoomTo(Math.round(map.zoom) + 1);
        else if (t === "-" || t === "_") map.zoomTo(Math.round(map.zoom) - 1);
        else if (t === "f") toggleFollow();
        else if (t === "c") centerOnBoat();
        else if (t === "i") {
            var p = map.mapToItem(surface, map.width / 2, map.height / 2);
            query(map.centerLat, map.centerLon, p.x, p.y);
        }
        else if (t === "w") {
            if (map.hovering) setWaypoint(map.hoverLat, map.hoverLon);
            else setWaypoint(map.centerLat, map.centerLon);
        }
        else if (t === "W") { waypoint = null; }
        else if (t === "n") toggleNight();
        else if (t === "b") toggleWind();
        else if (t === "s") toggleStream();
        else if (t === "t") toggleTimeBar();
        else if (t === "]") stepWind(1);
        else if (t === "[") stepWind(-1);
        else if (t === " ") togglePlay();
        else if (t === "?") sheet.visible = true;
        else if (t === "q") dismiss();
        else return false;
        saveSoon.restart();
        return true;
    }

    // For checks and captures, where no keyboard can be driven:
    //   quickshell ipc -p ui/shell.qml call omahelm view 37.86 -122.33 14
    IpcHandler {
        target: "omahelm"
        function view(lat: real, lon: real, zoom: real): void {
            app.follow = false;
            map.zoom = Math.max(map.minZoom, Math.min(map.maxZoom, zoom));
            map.lookAt(lat, lon);
            app.placed = true;
        }
        function press(key: string): void { app.run(key); }
        // A click at a point of the map, left (`query`) or right (`waypoint`).
        function click(x: real, y: real, button: string): void {
            var lat = Geo.lat(map.cy + (y - map.height / 2) / map.world);
            var lon = Geo.lon(map.cx + (x - map.width / 2) / map.world);
            if (button === "right") app.setWaypoint(lat, lon);
            else {
                var p = map.mapToItem(surface, x, y);
                app.query(lat, lon, p.x, p.y);
            }
        }
        function hover(x: real, y: real): void { map.hover = Qt.point(x, y); map.hovering = true; }
        // The scrubber, `hours` on from now, and play.
        function scrub(hours: int): void { app.windPlaying = false; app.scrubTo(hours); }
        function play(): void { app.togglePlay(); }
        function status(): string {
            return JSON.stringify({lat: map.centerLat, lon: map.centerLon, zoom: map.zoom, follow: app.follow,
                                   level: map.level, shown: map.shownLevel, tiles: Object.keys(map.tiles).length,
                                   engine: app.helm.connected, charts: app.helm.state ? app.helm.state.charts.status : "",
                                   keel: app.keel.connected, fix: app.fix ? app.fix.status : "", card: app.cardOpen,
                                   features: app.result ? app.result.features.length : -1, waypoint: app.waypoint,
                                   wind: app.windOn, windHours: app.windHours, windSpan: app.windSpan,
                                   playing: app.windPlaying, cached: Object.keys(app.windCache).length,
                                   timeBar: app.timeBar, timeBarShown: scrubber.visible,
                                   scrub: app.scrubText,
                                   night: app.theme.night, nightTiles: map.nightTiles,
                                   barbs: app.windField ? app.windField.points.length : -1,
                                   stations: app.windStations.length,
                                   cardWind: app.windOn && app.cardOpen ? app.cardForecastText : "",
                                   cardStation: app.cardStation ? app.cardStation.station.id : "",
                                   hoverStation: map.hoverStation ? map.hoverStation.id : "",
                                   stream: app.streamOn, streams: app.streamArrows.length,
                                   streamsShown: map.shownStreams, scrubSpan: app.scrubSpan,
                                   streamCurve: app.streamCurve ? app.streamCurve.values.length : -1,
                                   streamCurveAt: app.streamCurve ? app.streamCurve.name : "",
                                   streamEngine: app.stream.connected, streamText: app.streamText,
                                   hoverStream: map.hoverStream ? map.hoverStream.station : "",
                                   cursor: app.cursorText});
        }
    }

    // ------------------------------------------------------------ window

    FloatingWindow {
        id: win
        title: "Omahelm"
        visible: app.opened
        onVisibleChanged: {
            if (!visible && app.opened) app.dismiss();
            else if (visible) Qt.callLater(() => surface.forceActiveFocus());
        }
        implicitWidth: Number(Quickshell.env("OMAHELM_WIDTH")) || 1200
        implicitHeight: Number(Quickshell.env("OMAHELM_HEIGHT")) || 800
        color: app.theme.background

        // Plain text: labels carry what the engines send, like a station's
        // name, and that mustn't be read as markup.
        component Label: Text {
            textFormat: Text.PlainText
            color: app.theme.foreground
            font.family: app.theme.font
            font.pixelSize: app.theme.baseSize
            elide: Text.ElideRight
            maximumLineCount: 1
        }

        Item {
            id: surface
            anchors.fill: parent
            focus: true
            Keys.onPressed: e => app.key(e)

            ChartMap {
                id: map
                anchors { left: parent.left; right: parent.right; top: parent.top; bottom: statusBar.top }
                helm: app.helm
                theme: app.theme
                fix: app.fix
                targets: app.keel.targets
                targetsAt: app.keel.targetsAt
                now: app.keel.now
                track: app.track
                waypoint: app.waypoint
                wind: app.windField
                stations: app.windStations
                streams: app.streamArrows
                onPointed: (lat, lon, x, y, action) => {
                    if (action === "waypoint") app.setWaypoint(lat, lon);
                    else app.query(lat, lon, x, y);
                }
                onPanned: app.follow = false
                onWidthChanged: app.placeCamera()
                onCxChanged: saveSoon.restart()
                onZoomChanged: saveSoon.restart()
            }

            // Follow and help, top right.
            Row {
                anchors { top: parent.top; right: parent.right; margins: 10 }
                spacing: 6
                z: 30
                Rectangle {
                    height: 24
                    width: followLabel.implicitWidth + 16
                    color: app.follow ? app.theme.accent : Qt.alpha(app.theme.background, 0.85)
                    border.width: 1
                    border.color: app.follow ? app.theme.accent : Qt.alpha(app.theme.foreground, 0.25)
                    Label {
                        id: followLabel
                        anchors.centerIn: parent
                        text: app.follow ? "FOLLOWING  f" : "FOLLOW  f"
                        color: app.follow ? app.theme.background : app.theme.foreground
                        font.pixelSize: app.theme.baseSize - 1
                    }
                    MouseArea { anchors.fill: parent; onClicked: app.toggleFollow() }
                }
                Rectangle {
                    height: 24
                    width: nightLabel.implicitWidth + 16
                    color: app.theme.night ? app.theme.accent : Qt.alpha(app.theme.background, 0.85)
                    border.width: 1
                    border.color: app.theme.night ? app.theme.accent : Qt.alpha(app.theme.foreground, 0.25)
                    Label {
                        id: nightLabel
                        anchors.centerIn: parent
                        text: "NIGHT  n"
                        color: app.theme.night ? app.theme.background : app.theme.foreground
                        font.pixelSize: app.theme.baseSize - 1
                    }
                    MouseArea { anchors.fill: parent; onClicked: app.toggleNight() }
                }
                Rectangle {
                    height: 24
                    width: 28
                    color: Qt.alpha(app.theme.background, 0.85)
                    border.width: 1
                    border.color: Qt.alpha(app.theme.foreground, 0.25)
                    Label { anchors.centerIn: parent; text: "?"; font.pixelSize: app.theme.baseSize - 1 }
                    MouseArea { anchors.fill: parent; onClicked: sheet.visible = true }
                }
            }

            // The app's name, then the wind layer: which hour, from which
            // run, and the chip that opens the time bar.
            Row {
                z: 30
                anchors { top: parent.top; left: parent.left; margins: 10 }
                spacing: 6
                // The name is known at a glance, as in omalookout and
                // omawind. It sits on a chip, not bare like theirs: the
                // chart runs under it, and plain text would be lost over
                // pale shoal water.
                Rectangle {
                    height: 24
                    width: nameLabel.implicitWidth + 16
                    color: Qt.alpha(app.theme.background, 0.85)
                    border.width: 1
                    border.color: Qt.alpha(app.theme.foreground, 0.25)
                    Label {
                        id: nameLabel
                        anchors.centerIn: parent
                        text: "OMAHELM"
                        color: app.theme.accent
                        font.bold: true
                        font.pixelSize: app.theme.baseSize - 1
                    }
                }
                Rectangle {
                    visible: app.windOn
                    height: 24
                    width: windLabel.implicitWidth + 16
                    color: Qt.alpha(app.theme.background, 0.85)
                    border.width: 1
                    border.color: Qt.alpha(app.theme.foreground, 0.25)
                    Label {
                        id: windLabel
                        anchors.centerIn: parent
                        text: app.windText
                        font.pixelSize: app.theme.baseSize - 1
                    }
                }
                Rectangle {
                    visible: app.streamOn
                    height: 24
                    width: streamLabel.implicitWidth + 16
                    color: Qt.alpha(app.theme.background, 0.85)
                    border.width: 1
                    border.color: Qt.alpha(app.theme.foreground, 0.25)
                    Label {
                        id: streamLabel
                        anchors.centerIn: parent
                        text: app.streamText
                        font.pixelSize: app.theme.baseSize - 1
                    }
                }
                Rectangle {
                    visible: app.windOn || app.streamOn
                    height: 24
                    width: timeLabel.implicitWidth + 16
                    color: app.timeBar ? app.theme.accent : Qt.alpha(app.theme.background, 0.85)
                    border.width: 1
                    border.color: app.timeBar ? app.theme.accent : Qt.alpha(app.theme.foreground, 0.25)
                    Label {
                        id: timeLabel
                        anchors.centerIn: parent
                        text: "TIME  t"
                        color: app.timeBar ? app.theme.background : app.theme.foreground
                        font.pixelSize: app.theme.baseSize - 1
                    }
                    MouseArea { anchors.fill: parent; onClicked: { app.toggleTimeBar(); saveSoon.restart(); } }
                }
            }

            // The wind over time: the forecast at the boat hour by hour, an
            // hour to drag or click to, and play. Only when opened, with the
            // barbs on and forecast hours ahead.
            Rectangle {
                id: scrubber
                // Too narrow a window for a bar of hours: none.
                visible: app.timeBar && (app.windOn || app.streamOn) && app.scrubSpan > 0
                         && app.notice === "" && map.width >= 240
                z: 30
                anchors { left: parent.left; right: parent.right; bottom: statusBar.top; margins: 10 }
                height: app.theme.baseSize * 2 + 36
                color: Qt.alpha(app.theme.background, 0.88)
                border.width: 1
                border.color: Qt.alpha(app.theme.foreground, 0.25)
                // Clicks between the controls don't reach the chart.
                MouseArea { anchors.fill: parent }

                Rectangle {
                    id: playButton
                    anchors { left: parent.left; leftMargin: 10; verticalCenter: parent.verticalCenter }
                    width: 30
                    height: 30
                    color: app.windPlaying ? app.theme.accent : "transparent"
                    border.width: 1
                    border.color: app.windPlaying ? app.theme.accent : Qt.alpha(app.theme.foreground, 0.35)
                    Label {
                        anchors.centerIn: parent
                        text: app.windPlaying ? "❚❚" : "▶"
                        color: app.windPlaying ? app.theme.background : app.theme.foreground
                        font.pixelSize: app.theme.baseSize - 1
                    }
                    MouseArea { anchors.fill: parent; onClicked: app.togglePlay() }
                }

                Column {
                    id: readout
                    // Only with room for it and a bar of hours beside it.
                    visible: scrubber.width >= width + 220
                    anchors { right: parent.right; rightMargin: 12; verticalCenter: parent.verticalCenter }
                    width: app.theme.baseSize * 16
                    spacing: 2
                    Label {
                        width: parent.width
                        horizontalAlignment: Text.AlignRight
                        text: app.scrubText
                        font.bold: true
                    }
                    Label {
                        width: parent.width
                        horizontalAlignment: Text.AlignRight
                        text: app.scrubWhere
                        color: Qt.alpha(app.theme.foreground, 0.65)
                        font.pixelSize: app.theme.baseSize - 2
                    }
                }

                Item {
                    id: hoursBar
                    anchors { left: playButton.right; leftMargin: 14
                              right: readout.visible ? readout.left : parent.right; rightMargin: readout.visible ? 18 : 12
                              top: parent.top; bottom: parent.bottom; topMargin: 6; bottomMargin: 4 }
                    readonly property real step: app.scrubSpan > 0 ? width / app.scrubSpan : width

                    // The forecast wind at the boat, speed shaded and gusts
                    // dashed, over an hour's tick each: a label every few,
                    // and the day at local midnight.
                    Canvas {
                        id: timeline
                        anchors.fill: parent
                        // Painted colors don't follow the theme on their own.
                        property color ink: app.theme.foreground
                        property color accent: app.theme.accent
                        onInkChanged: requestPaint()
                        onAccentChanged: requestPaint()
                        onWidthChanged: requestPaint()
                        onHeightChanged: requestPaint()
                        property color flood: app.theme.accent
                        property color ebb: app.theme.red
                        onFloodChanged: requestPaint()
                        onEbbChanged: requestPaint()
                        Connections {
                            target: app
                            function onWindOutlookChanged() { timeline.requestPaint(); }
                            function onScrubSpanChanged() { timeline.requestPaint(); }
                            function onStreamCurveChanged() { timeline.requestPaint(); }
                            function onMinuteChanged() { timeline.requestPaint(); }
                        }

                        // The stream over the same hours: flood above the
                        // line it slacks on, ebb below, with a tick at each
                        // slack.
                        //
                        // Shaded when it has the graph to itself. With the
                        // wind on as well its shading would fight the
                        // wind's, so it keeps only the line, the slack
                        // water and the ticks — which is all you need to
                        // find the slack you want.
                        function paintStream(ctx, span, step, base, graph, shade) {
                            var c = app.streamCurve;
                            if (!c || !c.values.length) return;
                            var start = Date.parse(c.start), inc = c.stepSeconds * 1000;
                            var most = 0.2, n;
                            for (n = 0; n < c.values.length; n++) most = Math.max(most, Math.abs(c.values[n]));
                            var mid = graph / 2;
                            function x(t) { return (t - base) / 3600e3 * step; }
                            function y(kn) { return mid - kn / most * (mid - 2); }
                            // The water it slacks on.
                            ctx.globalAlpha = 0.35;
                            ctx.strokeStyle = String(ink);
                            ctx.lineWidth = 1;
                            ctx.beginPath();
                            ctx.moveTo(0, Math.round(mid) + 0.5);
                            ctx.lineTo(span * step, Math.round(mid) + 0.5);
                            ctx.stroke();
                            // Flood and ebb, each shaded from that line.
                            for (var side = 0; shade && side < 2; side++) {
                                var up = side === 0;
                                ctx.beginPath();
                                ctx.moveTo(x(start), mid);
                                for (n = 0; n < c.values.length; n++) {
                                    var v = c.values[n];
                                    ctx.lineTo(x(start + n * inc), y(up ? Math.max(0, v) : Math.min(0, v)));
                                }
                                ctx.lineTo(x(start + (c.values.length - 1) * inc), mid);
                                ctx.closePath();
                                ctx.globalAlpha = 0.22;
                                ctx.fillStyle = up ? String(flood) : String(ebb);
                                ctx.fill();
                            }
                            ctx.globalAlpha = 0.8;
                            ctx.beginPath();
                            for (n = 0; n < c.values.length; n++) {
                                var px = x(start + n * inc), py = y(c.values[n]);
                                if (n === 0) ctx.moveTo(px, py);
                                else ctx.lineTo(px, py);
                            }
                            ctx.strokeStyle = String(ink);
                            ctx.lineWidth = 1.5;
                            ctx.stroke();
                            // Slack is the hour you are looking for.
                            ctx.globalAlpha = 0.9;
                            ctx.lineWidth = 1;
                            for (n = 0; n < c.turns.length; n++) {
                                if (c.turns[n].turn !== "slack") continue;
                                var tx = x(Date.parse(c.turns[n].time));
                                if (tx < 0 || tx > span * step) continue;
                                ctx.beginPath();
                                ctx.moveTo(Math.round(tx) + 0.5, mid - 5);
                                ctx.lineTo(Math.round(tx) + 0.5, mid + 5);
                                ctx.stroke();
                            }
                            ctx.globalAlpha = 1;
                        }
                        onPaint: {
                            var ctx = getContext("2d");
                            ctx.reset();
                            var span = app.scrubSpan, step = hoursBar.step, base = app.hourNow();
                            if (span <= 0 || width <= 0) return;
                            var fontPx = Math.max(9, app.theme.baseSize - 2);
                            var graph = Math.max(10, height - fontPx - 12);
                            var tickY = graph + 2;
                            if (app.streamOn) timeline.paintStream(ctx, span, step, base, graph, !app.windOn);
                            var hours = [], n, i;
                            for (n = 0; n < app.windOutlook.length; n++) {
                                var o = app.windOutlook[n];
                                i = (Date.parse(o.time) - base) / 3600e3;
                                if (i >= 0 && i <= span)
                                    hours.push({x: i * step, kn: o.speedKn, gust: typeof o.gustKn === "number" ? o.gustKn : null});
                            }
                            var most = 10;
                            for (n = 0; n < hours.length; n++) most = Math.max(most, hours[n].kn, hours[n].gust || 0);
                            function y(kn) { return graph - kn / most * (graph - 2); }
                            if (app.windOn && hours.length > 1) {
                                ctx.beginPath();
                                ctx.moveTo(hours[0].x, graph);
                                for (n = 0; n < hours.length; n++) ctx.lineTo(hours[n].x, y(hours[n].kn));
                                ctx.lineTo(hours[hours.length - 1].x, graph);
                                ctx.closePath();
                                ctx.globalAlpha = 0.25;
                                ctx.fillStyle = String(accent);
                                ctx.fill();
                                ctx.globalAlpha = 1;
                                ctx.beginPath();
                                for (n = 0; n < hours.length; n++) {
                                    if (n === 0) ctx.moveTo(hours[n].x, y(hours[n].kn));
                                    else ctx.lineTo(hours[n].x, y(hours[n].kn));
                                }
                                ctx.strokeStyle = String(accent);
                                ctx.lineWidth = 1.5;
                                ctx.stroke();
                                ctx.beginPath();
                                var drawing = false;
                                for (n = 0; n < hours.length; n++) {
                                    if (hours[n].gust === null) { drawing = false; continue; }
                                    if (drawing) ctx.lineTo(hours[n].x, y(hours[n].gust));
                                    else ctx.moveTo(hours[n].x, y(hours[n].gust));
                                    drawing = true;
                                }
                                ctx.globalAlpha = 0.6;
                                ctx.strokeStyle = String(ink);
                                ctx.lineWidth = 1;
                                if (ctx.setLineDash) ctx.setLineDash([2, 3]);
                                ctx.stroke();
                                if (ctx.setLineDash) ctx.setLineDash([]);
                                ctx.globalAlpha = 1;
                            }
                            var every = 24, choices = [1, 2, 3, 6, 12];
                            for (n = 0; n < choices.length; n++) {
                                if (choices[n] * step >= fontPx * 3.6) { every = choices[n]; break; }
                            }
                            ctx.strokeStyle = String(ink);
                            ctx.fillStyle = String(ink);
                            ctx.lineWidth = 1;
                            ctx.textBaseline = "top";
                            for (i = 0; i <= span; i++) {
                                var t = new Date(base + i * 3600e3), hr = t.getHours();
                                var x = Math.round(i * step) + 0.5, midnight = hr === 0;
                                ctx.globalAlpha = midnight ? 0.8 : 0.45;
                                ctx.beginPath();
                                ctx.moveTo(x, tickY);
                                ctx.lineTo(x, tickY + (midnight ? 8 : hr % every === 0 ? 5 : 3));
                                ctx.stroke();
                                var label = i === 0 ? "now" : midnight ? Qt.formatDateTime(t, "ddd")
                                    : hr % every === 0 ? String(hr).padStart(2, "0") : "";
                                // Kept clear of "now".
                                if (!label || (i > 0 && i * step < fontPx * 3)) continue;
                                ctx.globalAlpha = midnight || i === 0 ? 0.9 : 0.6;
                                ctx.font = (midnight || i === 0 ? "bold " : "") + fontPx + "px '" + app.theme.font + "'";
                                ctx.textAlign = i === 0 ? "left" : x > width - fontPx * 2 ? "right" : "center";
                                ctx.fillText(label, x, tickY + 9);
                            }
                            ctx.globalAlpha = 1;
                        }
                    }
                    // The hour chosen.
                    Rectangle {
                        x: app.windHours * hoursBar.step - 1.5
                        width: 3
                        height: hoursBar.height
                        color: app.theme.accent
                    }
                    // Drag or click to an hour; the wheel steps them. A little
                    // wider than the track, so its ends are easy to grab.
                    MouseArea {
                        anchors { fill: parent; leftMargin: -8; rightMargin: -8 }
                        preventStealing: true
                        function hourAt(x) { return (x - 8) / hoursBar.step; }
                        onPressed: m => {
                            app.windPlaying = false;
                            app.scrubTo(hourAt(m.x));
                        }
                        onPositionChanged: m => { if (pressed) app.scrubTo(hourAt(m.x)); }
                        onWheel: w => {
                            app.windPlaying = false;
                            app.scrubTo(app.windHours + (w.angleDelta.y > 0 ? -1 : 1));
                        }
                    }
                }
            }

            // No engine, no charts, or charts still being read.
            Rectangle {
                visible: app.notice !== ""
                z: 25
                anchors.centerIn: map
                width: Math.min(map.width - 40, noticeText.implicitWidth + 48)
                height: noticeText.implicitHeight + 40
                color: Qt.alpha(app.theme.background, 0.94)
                border.width: 1
                border.color: Qt.alpha(app.theme.foreground, 0.25)
                Text {
                    id: noticeText
                    anchors.centerIn: parent
                    width: Math.min(implicitWidth, map.width - 88)
                    text: app.notice
                    color: app.theme.foreground
                    font.family: app.theme.font
                    font.pixelSize: app.theme.baseSize + 1
                    wrapMode: Text.Wrap
                    lineHeight: 1.15
                }
            }

            Rectangle {
                visible: app.notice === "" && app.offChart && map.chartsReady
                z: 24
                anchors { horizontalCenter: map.horizontalCenter; top: map.top; topMargin: 12 }
                width: offLabel.implicitWidth + 20
                height: 26
                color: Qt.alpha(app.theme.background, 0.9)
                border.width: 1
                border.color: Qt.alpha(app.theme.foreground, 0.25)
                Label { id: offLabel; anchors.centerIn: parent; text: "No charts here" }
            }

            // A passing message: a bad request, no fix to follow.
            Rectangle {
                visible: app.toastText !== ""
                z: 31
                // Above the scrubber when it's up.
                anchors { horizontalCenter: map.horizontalCenter; bottom: scrubber.visible ? scrubber.top : map.bottom
                          bottomMargin: scrubber.visible ? 8 : 14 }
                width: Math.min(map.width - 40, toastLabel.implicitWidth + 24)
                height: 28
                color: app.theme.background
                border.width: 1
                border.color: app.theme.accent
                Label { id: toastLabel; anchors.centerIn: parent; width: Math.min(implicitWidth, parent.width - 24); text: app.toastText }
            }

            // What's charted where the click was.
            Rectangle {
                id: card
                visible: app.cardOpen
                z: 32
                width: Math.min(380, map.width - 20)
                height: Math.min(cardColumn.implicitHeight + 20, map.height - 20)
                x: Math.max(10, Math.min(app.cardAt.x + 14, map.width - width - 10))
                y: Math.max(10, Math.min(app.cardAt.y + 14, map.height - height - 10))
                color: app.theme.background
                border.width: 1
                border.color: Qt.alpha(app.theme.foreground, 0.3)
                clip: true
                MouseArea { anchors.fill: parent; onClicked: app.closeCard() }
                Column {
                    id: cardColumn
                    x: 10
                    y: 10
                    width: parent.width - 20
                    spacing: 8
                    // The wind there, with the barbs on: the forecast for the
                    // hour on show, then the nearest station's, for now.
                    Column {
                        visible: app.windOn
                        width: cardColumn.width
                        spacing: 1
                        Label {
                            width: parent.width
                            text: "Wind forecast  ·  HRRR"
                            color: Qt.alpha(app.theme.foreground, 0.65)
                            font.pixelSize: app.theme.baseSize - 2
                        }
                        Text {
                            width: parent.width
                            text: app.cardForecastText
                            textFormat: Text.PlainText
                            color: app.theme.foreground
                            font.family: app.theme.font
                            font.pixelSize: app.theme.baseSize
                            font.bold: true
                            wrapMode: Text.Wrap
                        }
                        Label {
                            width: parent.width
                            visible: text !== ""
                            text: app.cardForecastWhen
                            font.pixelSize: app.theme.baseSize - 1
                        }
                    }
                    Column {
                        visible: app.windOn && !!app.cardStation
                        width: cardColumn.width
                        spacing: 1
                        Label {
                            width: parent.width
                            text: {
                                var s = app.cardStation ? app.cardStation.station : null;
                                return "Measured  ·  " + (!s ? "" : s.name ? s.name + " (" + s.id + ")" : s.id);
                            }
                            color: Qt.alpha(app.theme.foreground, 0.65)
                            font.pixelSize: app.theme.baseSize - 2
                        }
                        Label {
                            width: parent.width
                            text: app.cardStation ? app.windWords(app.cardStation.station) : ""
                            font.bold: true
                        }
                        Label {
                            width: parent.width
                            text: app.cardStationWhere
                            font.pixelSize: app.theme.baseSize - 1
                        }
                    }
                    Rectangle {
                        visible: app.windOn
                        width: cardColumn.width
                        height: 1
                        color: Qt.alpha(app.theme.foreground, 0.18)
                    }
                    Label {
                        visible: !app.result
                        text: "Looking…"
                        color: Qt.alpha(app.theme.foreground, 0.65)
                    }
                    Label {
                        visible: !!app.result && app.result.features.length === 0
                        text: app.result && app.result.lost ? "Lost the chart engine. Click again once it's back." : "Nothing charted here."
                    }
                    Repeater {
                        model: app.result ? app.result.features.slice(0, 8) : []
                        Column {
                            required property var modelData
                            width: cardColumn.width
                            spacing: 1
                            Text {
                                width: parent.width
                                text: modelData.kind + (modelData.chart ? "  ·  " + modelData.chart : "")
                                color: Qt.alpha(app.theme.foreground, 0.65)
                                font.family: app.theme.font
                                font.pixelSize: app.theme.baseSize - 2
                                elide: Text.ElideRight
                            }
                            Text {
                                width: parent.width
                                text: modelData.title + (modelData.label ? "   " + modelData.label : "")
                                color: app.theme.foreground
                                font.family: app.theme.font
                                font.pixelSize: app.theme.baseSize
                                font.bold: true
                                wrapMode: Text.Wrap
                            }
                            Repeater {
                                model: modelData.lines || []
                                Text {
                                    required property var modelData
                                    width: parent.width
                                    text: modelData
                                    color: app.theme.foreground
                                    font.family: app.theme.font
                                    font.pixelSize: app.theme.baseSize - 1
                                    wrapMode: Text.Wrap
                                }
                            }
                        }
                    }
                }
            }

            // The keys.
            Rectangle {
                id: sheet
                visible: false
                z: 40
                anchors.centerIn: parent
                width: Math.min(parent.width - 40, 520)
                height: sheetColumn.implicitHeight + 32
                color: app.theme.background
                border.width: 1
                border.color: app.theme.accent
                MouseArea { anchors.fill: parent; onClicked: sheet.visible = false }
                Column {
                    id: sheetColumn
                    x: 16
                    y: 16
                    width: parent.width - 32
                    spacing: 4
                    Label { text: "OMAHELM KEYS"; color: app.theme.accent; font.bold: true }
                    Item { width: 1; height: 6 }
                    Repeater {
                        model: [
                            ["h j k l  arrows", "pan"],
                            ["+  −  wheel", "zoom"],
                            ["drag", "pan"],
                            ["click  i", "what's charted here, and its wind (i: the center)"],
                            ["f", "follow the boat"],
                            ["c", "center on the boat"],
                            ["w  right-click", "waypoint at the cursor"],
                            ["W", "clear the waypoint"],
                            ["b", "wind barbs from omawind: forecast, and measured on dots"],
                            ["s", "tidal stream from omatide: an arrow at each station"],
                            ["[  ]", "an hour earlier, later"],
                            ["t", "the time bar: the wind and the stream by the hour"],
                            ["space", "play the hours one after another"],
                            ["n", "Night Watch: red on black"],
                            ["Esc", "close the card"],
                            ["?", "these keys"],
                            ["q", "close"]
                        ]
                        Row {
                            required property var modelData
                            spacing: 12
                            Label { width: 150; text: modelData[0]; color: app.theme.accent }
                            Label { width: sheetColumn.width - 162; text: modelData[1] }
                        }
                    }
                    Item { width: 1; height: 6 }
                    Label {
                        width: sheetColumn.width
                        visible: !!app.helm.state && !!app.helm.state.problems
                        text: app.helm.state && app.helm.state.problems ? "⚠ " + app.helm.state.problems.join("  ·  ") : ""
                        color: app.theme.yellow
                        wrapMode: Text.Wrap
                        maximumLineCount: 6
                    }
                    Label {
                        width: sheetColumn.width
                        text: "Not for navigation. Carry a backup."
                        color: Qt.alpha(app.theme.foreground, 0.65)
                    }
                }
            }

            // Status bar: GPS, cursor, waypoint | scale.
            Rectangle {
                id: statusBar
                anchors { left: parent.left; right: parent.right; bottom: parent.bottom }
                height: app.theme.baseSize + 16
                color: app.theme.background
                Rectangle { anchors { left: parent.left; right: parent.right; top: parent.top } height: 1; color: Qt.alpha(app.theme.foreground, 0.18) }
                Row {
                    id: leftStatus
                    anchors { left: parent.left; leftMargin: 10; verticalCenter: parent.verticalCenter }
                    spacing: 22
                    width: parent.width - rightStatus.width - 30
                    clip: true
                    Label { id: gpsLabel; text: app.gpsText; color: app.gpsColor }
                    // What's left of the row after the fix, ellipsised rather
                    // than cut mid-glyph.
                    Label {
                        id: waypointLabel
                        visible: text !== "" && !map.hoverStation
                        text: app.waypointText
                        color: app.theme.accent
                        width: Math.min(implicitWidth, Math.max(0, leftStatus.width - gpsLabel.width - leftStatus.spacing))
                        elide: Text.ElideRight
                    }
                    // The cursor gives way to the waypoint, but a station
                    // pointed at takes the waypoint's place.
                    Label {
                        visible: text !== "" && (!app.waypoint || !!map.hoverStation)
                        text: app.cursorText
                        color: Qt.alpha(app.theme.foreground, 0.7)
                        width: Math.min(implicitWidth, Math.max(0, leftStatus.width - gpsLabel.width - leftStatus.spacing))
                        elide: Text.ElideRight
                    }
                }
                Row {
                    id: rightStatus
                    anchors { right: parent.right; rightMargin: 10; verticalCenter: parent.verticalCenter }
                    spacing: 10
                    Item {
                        width: app.barPx
                        height: 10
                        anchors.verticalCenter: parent.verticalCenter
                        Rectangle { anchors { left: parent.left; right: parent.right; bottom: parent.bottom } height: 2; color: app.theme.foreground }
                        Rectangle { anchors { left: parent.left; bottom: parent.bottom } width: 2; height: 8; color: app.theme.foreground }
                        Rectangle { anchors { right: parent.right; bottom: parent.bottom } width: 2; height: 8; color: app.theme.foreground }
                    }
                    Label { text: Geo.nmText(app.barNm) }
                    Label { visible: statusBar.width > 700; text: Geo.scaleText(map.zoom, map.centerLat); color: Qt.alpha(app.theme.foreground, 0.7) }
                    Label { visible: app.units !== "" && statusBar.width > 1000; text: "depths " + app.units; color: Qt.alpha(app.theme.foreground, 0.7) }
                }
            }
        }
    }

    Component.onCompleted: Qt.callLater(() => surface.forceActiveFocus())
}
