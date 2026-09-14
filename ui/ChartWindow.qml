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
        if (!helm.connected) { result = {features: [], lost: true}; return; }
        helm.send({type: "query", id: queryId, lat: lat, lon: lon, zoom: Math.round(map.zoom)});
    }
    function closeCard() {
        cardOpen = false;
        map.mark = null;
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
        function onStateChanged() { app.placeCamera(); }
        // A question the engine can no longer answer.
        function onConnectedChanged() { if (!app.helm.connected && app.cardOpen && app.result === null) app.result = {features: [], lost: true}; }
    }

    // ---------------------------------------------------------- the wind

    // omawind's wind barbs, now or at a whole hour ahead: b, [ and ].
    property bool windOn: false
    // The hour chosen, as UTC milliseconds, or 0 for now. It's absolute,
    // so the barbs stay right as the clock turns over.
    property real windAt: 0
    property int windId: 0
    property var windField: null    // the last `field` asked for
    property string windError: ""
    // Read by the bindings below that follow the clock.
    property real minute: Date.now()
    Timer { interval: 30000; repeat: true; running: app.windOn; onTriggered: app.minute = Date.now() }

    function hourNow() { return Math.floor(Date.now() / 3600e3) * 3600e3; }
    function windLast() {
        var f = wind.forecast;
        var t = f && typeof f.last === "string" ? Date.parse(f.last) : NaN;
        return isNaN(t) ? 0 : t;
    }
    // Hours from the hour under way to the one chosen.
    readonly property int windHours: {
        void app.minute;
        return windAt > 0 ? Math.round((windAt - hourNow()) / 3600e3) : 0;
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
        if (!windOn) windAt = 0;
        else windSettle.restart();
    }
    // An hour the clock has reached is now; one past the forecast's end is
    // its last.
    function clampWind(at) {
        var now = hourNow(), last = windLast();
        if (at > 0 && last && at > last) at = last;
        return at > now ? at : 0;
    }
    function stepWind(d) {
        if (!windOn) windOn = true;
        var next = clampWind((windAt > 0 ? windAt : hourNow()) + d * 3600e3);
        if (next !== windAt) {
            windAt = next;
            forgetWind();
        }
        windSettle.restart();
    }
    // The view and a margin around it, thinned to a barb every 70 pixels
    // or so.
    function requestWind() {
        if (!windOn || !wind.connected || map.width <= 0 || map.height <= 0) return;
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
        var w = map.width / map.world, h = map.height / map.world;
        function clampLat(v) { return Math.max(-85, Math.min(85, v)); }
        var north = clampLat(Geo.lat(map.cy - h * 0.6)), south = clampLat(Geo.lat(map.cy + h * 0.6));
        var west = Math.max(-180, Geo.lon(map.cx - w * 0.6)), east = Math.min(180, Geo.lon(map.cx + w * 0.6));
        if (!(south < north && west < east)) return;
        var request = {
            type: "field", id: ++windId, south: south, west: west, north: north, east: east,
            max: Math.max(1, Math.min(2000, Math.floor(map.width * map.height / 4900)))
        };
        if (windAt > 0) request.time = new Date(windAt).toISOString().slice(0, 19) + "Z";
        wind.send(request);
    }
    Timer { id: windSettle; interval: 150; onTriggered: app.requestWind() }
    Connections {
        target: app.wind
        function onField(m) {
            if (m.id !== app.windId) return;
            app.windField = m;
            app.windError = "";
        }
        // Said once, not at every minute's new request.
        function onRejected(m) {
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
    readonly property string windText: {
        void app.minute;
        if (wind.incompatible) return "WIND  omawind speaks a newer protocol: update omahelm";
        if (!wind.connected) return "WIND  omawind isn't running";
        var f = wind.forecast;
        if (!f || f.status === "none") return "WIND  no forecast yet";
        // The run the barbs on show came from, once they're in.
        var run = windField && typeof windField.run === "string" ? windField.run : f.run;
        var runText = "HRRR " + Qt.formatDateTime(new Date(run), "HH:mm") + " run";
        if (f.status === "expired") return "WIND  the " + runText + " has run out";
        var when = windAt > 0 ? "+" + windHours + " h  " + Qt.formatDateTime(new Date(windAt), "ddd HH:mm") : "now";
        return "WIND " + when + "   " + runText + (f.status === "old" ? ", old" : "");
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
    readonly property real barNm: Geo.niceNm(110 * map.metresPerPixel / 1852)
    readonly property real barPx: barNm * 1852 / map.metresPerPixel

    // ------------------------------------------------------------ keys

    function key(e) {
        var t = e.key === Qt.Key_Escape ? "Escape"
            : e.key === Qt.Key_Left ? "h" : e.key === Qt.Key_Right ? "l"
            : e.key === Qt.Key_Up ? "k" : e.key === Qt.Key_Down ? "j" : e.text;
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
        else if (t === "b") toggleWind();
        else if (t === "]") stepWind(1);
        else if (t === "[") stepWind(-1);
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
        function status(): string {
            return JSON.stringify({lat: map.centerLat, lon: map.centerLon, zoom: map.zoom, follow: app.follow,
                                   level: map.level, shown: map.shownLevel, tiles: Object.keys(map.tiles).length,
                                   engine: app.helm.connected, charts: app.helm.state ? app.helm.state.charts.status : "",
                                   keel: app.keel.connected, fix: app.fix ? app.fix.status : "", card: app.cardOpen,
                                   features: app.result ? app.result.features.length : -1, waypoint: app.waypoint,
                                   wind: app.windOn, windHours: app.windHours,
                                   barbs: app.windField ? app.windField.points.length : -1});
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

        component Label: Text {
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
                    width: 28
                    color: Qt.alpha(app.theme.background, 0.85)
                    border.width: 1
                    border.color: Qt.alpha(app.theme.foreground, 0.25)
                    Label { anchors.centerIn: parent; text: "?"; font.pixelSize: app.theme.baseSize - 1 }
                    MouseArea { anchors.fill: parent; onClicked: sheet.visible = true }
                }
            }

            // The wind layer: which hour, from which run.
            Rectangle {
                visible: app.windOn
                z: 30
                anchors { top: parent.top; left: parent.left; margins: 10 }
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
                anchors { horizontalCenter: map.horizontalCenter; bottom: map.bottom; bottomMargin: 14 }
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
                            ["click  i", "what's charted here (i: at the centre)"],
                            ["f", "follow the boat"],
                            ["c", "centre on the boat"],
                            ["w  right-click", "waypoint at the cursor"],
                            ["W", "clear the waypoint"],
                            ["b", "wind barbs, from omawind"],
                            ["[  ]", "the wind an hour earlier, later"],
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
                        visible: text !== ""
                        text: app.waypointText
                        color: app.theme.accent
                        width: Math.min(implicitWidth, Math.max(0, leftStatus.width - gpsLabel.width - leftStatus.spacing))
                        elide: Text.ElideRight
                    }
                    // The cursor gives way to the waypoint.
                    Label {
                        visible: text !== "" && !app.waypoint
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
