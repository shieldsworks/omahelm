import QtQuick
import QtQuick.Shapes
import QtQuick.Window
import "Geo.js" as Geo

// The chart: the engine's tiles under a Web Mercator camera, with the
// boat, its track, AIS targets and the waypoint drawn over them. It holds
// no sockets; the window feeds it the engine and omakeel.
Item {
    id: map
    clip: true

    property var helm
    property var theme
    property var fix: null
    property var targets: []
    property real targetsAt: 0
    property real now: Date.now()
    property var track: []          // [{x, y}] in Mercator units, oldest first
    property var waypoint: null     // {lat, lon}
    property var mark: null         // {lat, lon}: the point a query asked about

    // Camera: the view centre in Mercator units (the world is the unit
    // square, x east, y south) and a fractional zoom level.
    property real cx: Geo.mercX(-122.3148)
    property real cy: Geo.mercY(37.8663)
    property real zoom: 13
    readonly property real minZoom: 3
    readonly property real maxZoom: 18
    readonly property real world: 256 * Math.pow(2, zoom)
    readonly property real centerLat: Geo.lat(cy)
    readonly property real centerLon: Geo.lon(cx)
    readonly property real metresPerPixel: Geo.metresPerPixel(zoom, centerLat)

    property point hover: Qt.point(0, 0)
    property bool hovering: false
    readonly property real hoverLat: Geo.lat(cy + (hover.y - height / 2) / world)
    readonly property real hoverLon: Geo.lon(cx + (hover.x - width / 2) / world)

    // A click that wasn't a drag: `query` (left) or `waypoint` (right).
    signal pointed(real lat, real lon, real x, real y, string action)
    // The user moved the camera by hand, which ends follow mode.
    signal panned()

    function sx(mx) { return (mx - cx) * world + width / 2; }
    function sy(my) { return (my - cy) * world + height / 2; }
    function px(lat, lon) { return Qt.point(sx(Geo.mercX(lon)), sy(Geo.mercY(lat))); }
    function setCenter(x, y) {
        cx = Math.max(0, Math.min(1, x));
        cy = Math.max(0, Math.min(1, y));
    }
    function lookAt(lat, lon) { setCenter(Geo.mercX(lon), Geo.mercY(lat)); }
    // One step is an eighth of the shorter side.
    function pan(dx, dy) {
        var step = Math.min(width, height) / 8 / world;
        setCenter(cx + dx * step, cy + dy * step);
    }
    // Zoom keeping the point under (x, y) where it is.
    function zoomAt(z, x, y) {
        zoomAnim.stop();
        z = Math.max(minZoom, Math.min(maxZoom, z));
        var mx = cx + (x - width / 2) / world, my = cy + (y - height / 2) / world;
        var w = 256 * Math.pow(2, z);
        zoom = z;
        setCenter(mx - (x - width / 2) / w, my - (y - height / 2) / w);
    }
    function zoomTo(z) {
        zoomAnim.stop();
        zoomAnim.to = Math.max(minZoom, Math.min(maxZoom, z));
        zoomAnim.start();
    }
    // The zoom that shows a lat/lon box whole.
    function fit(west, south, east, north) {
        var dx = Math.max(1e-9, Geo.mercX(east) - Geo.mercX(west));
        var dy = Math.max(1e-9, Geo.mercY(south) - Geo.mercY(north));
        setCenter((Geo.mercX(west) + Geo.mercX(east)) / 2, (Geo.mercY(north) + Geo.mercY(south)) / 2);
        var z = Math.log(Math.min(Math.max(1, width) / (dx * 256), Math.max(1, height) / (dy * 256))) / Math.LN2;
        zoom = Math.max(minZoom, Math.min(maxZoom, Math.floor(z * 4) / 4));
    }
    NumberAnimation { id: zoomAnim; target: map; property: "zoom"; duration: 160; easing.type: Easing.OutCubic }

    // ---------------------------------------------------------------- tiles

    readonly property int pixelRatio: Math.max(1, Math.min(4, Math.round(Screen.devicePixelRatio || 1)))
    readonly property var tileState: helm && helm.state ? helm.state.tiles : null
    readonly property string root: tileState ? tileState.root : ""
    readonly property string generation: tileState ? tileState.generation : ""
    readonly property bool chartsReady: !!(helm && helm.connected && helm.state && helm.state.charts
                                           && helm.state.charts.status === "ok")
    readonly property int level: Math.max(0, Math.min(18, Math.round(zoom)))
    property var tiles: ({})        // "z/x/y" → {path, ready}
    property var request: null      // the rectangle last asked for
    readonly property int requestLevel: request ? request.z : -1
    // The level whose tiles fill the view; the requested level draws over
    // it as its tiles arrive and replaces it once they're all in, so a zoom
    // never flashes blank.
    property int shownLevel: -1

    onGenerationChanged: reset()
    onChartsReadyChanged: reset()
    onPixelRatioChanged: reset()
    onCxChanged: settle.restart()
    onCyChanged: settle.restart()
    onZoomChanged: settle.restart()
    onWidthChanged: settle.restart()
    onHeightChanged: settle.restart()

    function reset() {
        tiles = ({});
        tileModel.clear();
        request = null;
        shownLevel = -1;
        settle.restart();
    }

    Timer { id: settle; interval: 50; onTriggered: map.requestTiles() }

    function tileRect(z) {
        var n = Math.pow(2, z), w = world;
        function clamp(v) { return Math.max(0, Math.min(n - 1, v)); }
        return {
            z: z,
            x0: clamp(Math.floor((cx - width / 2 / w) * n)), x1: clamp(Math.floor((cx + width / 2 / w) * n)),
            y0: clamp(Math.floor((cy - height / 2 / w) * n)), y1: clamp(Math.floor((cy + height / 2 / w) * n))
        };
    }
    function inside(z, x, y, r) {
        return !!r && z === r.z && x >= r.x0 && x <= r.x1 && y >= r.y0 && y <= r.y1;
    }

    function requestTiles() {
        if (!chartsReady || !root || width <= 0 || height <= 0) return;
        var r = tileRect(level);
        r.scale = pixelRatio;
        // The protocol's limit; only a wall of screens needs more.
        if ((r.x1 - r.x0 + 1) * (r.y1 - r.y0 + 1) > 256) return;
        var same = request && ["z", "x0", "y0", "x1", "y1", "scale"].every(k => request[k] === r[k]);
        if (!same) {
            request = r;
            // The engine answers tiles it already has at once, so the
            // whole rectangle is asked for every time.
            helm.send({type: "tiles", z: r.z, x0: r.x0, y0: r.y0, x1: r.x1, y1: r.y1, scale: r.scale});
        }
        rebuild();
    }

    function tileArrived(m) {
        if (!request || m.scale !== request.scale || !inside(m.z, m.x, m.y, request)) return;
        // A tile drawn before the theme, settings or charts changed.
        if (m.generation !== undefined && m.generation !== generation) return;
        var key = m.z + "/" + m.x + "/" + m.y;
        var old = tiles[key];
        if (m.error !== undefined) tiles[key] = {path: "", ready: true};
        else tiles[key] = {path: m.path, ready: !!old && old.path === m.path && old.ready};
        rebuild();
    }

    function imageReady(key, path) {
        var t = tiles[key];
        if (!t || t.path !== path || t.ready) return;
        // A file that failed to load counts too, or the old level would
        // hold the screen forever.
        t.ready = true;
        Qt.callLater(rebuild);
    }

    ListModel { id: tileModel }

    function rebuild() {
        if (!request) return;
        var complete = true;
        for (var y = request.y0; y <= request.y1 && complete; y++)
            for (var x = request.x0; x <= request.x1; x++) {
                var t = tiles[request.z + "/" + x + "/" + y];
                if (!t || !t.ready) { complete = false; break; }
            }
        if (complete || shownLevel < 0) shownLevel = request.z;
        var held = shownLevel === request.z ? null : tileRect(shownLevel);
        var keep = {}, wanted = {};
        for (var key in tiles) {
            var p = key.split("/"), z = Number(p[0]), col = Number(p[1]), row = Number(p[2]);
            if (!inside(z, col, row, request) && !(held && inside(z, col, row, held))) continue;
            keep[key] = tiles[key];
            if (tiles[key].path) wanted[key] = {key: key, lvl: z, col: col, row: row, path: tiles[key].path};
        }
        tiles = keep;
        // Delegates for tiles that stay are kept, so a pan never reloads
        // an image already on screen.
        for (var i = tileModel.count - 1; i >= 0; i--) {
            var e = tileModel.get(i);
            if (!wanted[e.key] || wanted[e.key].path !== e.path) { tileModel.remove(i); continue; }
            delete wanted[e.key];
        }
        for (var k in wanted) tileModel.append(wanted[k]);
    }

    Repeater {
        model: tileModel
        Image {
            required property string key
            required property int lvl
            required property int col
            required property int row
            required property string path
            readonly property real n: Math.pow(2, lvl)
            // Edges round to whole pixels so neighbours meet without a seam.
            readonly property real edgeX: Math.round(map.sx(col / n))
            readonly property real edgeY: Math.round(map.sy(row / n))
            x: edgeX
            y: edgeY
            width: Math.round(map.sx((col + 1) / n)) - edgeX
            height: Math.round(map.sy((row + 1) / n)) - edgeY
            z: lvl === map.requestLevel ? 1 : 0
            visible: status === Image.Ready
            source: path ? "file://" + map.root + path : ""
            asynchronous: true
            cache: false
            smooth: true
            onStatusChanged: if (status === Image.Ready || status === Image.Error) map.imageReady(key, path)
        }
    }

    // ------------------------------------------------------------- overlays

    readonly property var boat: fix && typeof fix.lat === "number" && typeof fix.lon === "number" ? fix : null
    readonly property bool boatLive: !!boat && fix.status === "ok"
    // The last course made good; kept while the boat is stopped.
    property real lastCog: 0
    onFixChanged: if (fix && typeof fix.cogDeg === "number" && (fix.sogKn || 0) >= 0.5) lastCog = fix.cogDeg
    readonly property point boatAt: boat ? px(boat.lat, boat.lon) : Qt.point(-10000, -10000)

    function trackPath() {
        var out = [];
        for (var i = 0; i < track.length; i++) out.push(Qt.point(sx(track[i].x), sy(track[i].y)));
        if (boat && out.length) out.push(boatAt);
        return out;
    }
    // Where the boat will be in six minutes on its course and speed.
    function predictor() {
        if (!boatLive || !(fix.sogKn >= 0.5) || typeof fix.cogDeg !== "number") return [];
        var end = Geo.destination(boat.lat, boat.lon, fix.cogDeg, fix.sogKn * 0.1);
        return [boatAt, px(end.lat, end.lon)];
    }
    function leg() {
        if (!boat || !waypoint) return [];
        return [boatAt, px(waypoint.lat, waypoint.lon)];
    }

    Shape {
        anchors.fill: parent
        z: 10
        preferredRendererType: Shape.CurveRenderer
        ShapePath {
            strokeColor: Qt.alpha(map.theme.accent, 0.7)
            strokeWidth: 2
            fillColor: "transparent"
            capStyle: ShapePath.RoundCap
            joinStyle: ShapePath.RoundJoin
            PathPolyline { path: map.track.length ? map.trackPath() : [] }
        }
        ShapePath {
            strokeColor: map.theme.accent
            strokeWidth: 2
            fillColor: "transparent"
            PathPolyline { path: map.predictor() }
        }
        ShapePath {
            strokeColor: map.theme.accent
            strokeWidth: 1.5
            strokeStyle: ShapePath.DashLine
            dashPattern: [4, 3]
            fillColor: "transparent"
            PathPolyline { path: map.leg() }
        }
    }

    // AIS targets: a triangle on the vessel's heading, a six-minute vector
    // on its course, red when omakeel judges it a danger.
    Repeater {
        model: map.targets.filter(t => typeof t.lat === "number" && typeof t.lon === "number")
        Item {
            id: target
            required property var modelData
            readonly property var t: modelData
            // Carried forward along its course and speed to now, as omakeel
            // does for CPA, so the target sits where the danger is judged.
            readonly property real age: (typeof t.ageSeconds === "number" ? t.ageSeconds : 0)
                                        + Math.max(0, (map.now - map.targetsAt) / 1000)
            // As omakeel does: any known course and speed, however slow.
            readonly property bool moving: typeof t.cogDeg === "number" && typeof t.sogKn === "number"
            readonly property var here: moving ? Geo.destination(t.lat, t.lon, t.cogDeg, t.sogKn * age / 3600) : ({lat: t.lat, lon: t.lon})
            readonly property point at: map.px(here.lat, here.lon)
            readonly property bool headed: typeof t.headingDeg === "number" && t.headingDeg < 360
            readonly property real course: typeof t.cogDeg === "number" ? t.cogDeg : headed ? t.headingDeg : 0
            readonly property real heading: headed ? t.headingDeg : course
            // No course, no vector: never a made-up one pointing north.
            readonly property real vector: moving ? t.sogKn * 0.1 * 1852 / map.metresPerPixel : 0
            readonly property color ink: t.danger ? map.theme.red : map.theme.foreground
            x: at.x
            y: at.y
            z: t.danger ? 12 : 11
            // A report more than three minutes old is shown faded.
            opacity: age > 180 ? 0.45 : 1
            visible: at.x > -80 && at.x < map.width + 80 && at.y > -80 && at.y < map.height + 80
            Item {
                rotation: target.course
                visible: target.vector > 2
                Shape {
                    preferredRendererType: Shape.CurveRenderer
                    ShapePath {
                        strokeColor: target.ink
                        strokeWidth: 1.5
                        fillColor: "transparent"
                        startX: 0; startY: 0
                        PathLine { x: 0; y: -target.vector }
                    }
                }
            }
            Item {
                rotation: target.heading
                Shape {
                    preferredRendererType: Shape.CurveRenderer
                    ShapePath {
                        strokeColor: target.ink
                        strokeWidth: 1.5
                        fillColor: Qt.alpha(target.ink, target.t.danger ? 0.5 : 0.2)
                        joinStyle: ShapePath.MiterJoin
                        startX: 0; startY: -9
                        PathLine { x: 6; y: 7 }
                        PathLine { x: -6; y: 7 }
                        PathLine { x: 0; y: -9 }
                    }
                }
            }
            Text {
                visible: target.t.danger || map.zoom >= 13
                x: 10
                y: -6
                text: typeof target.t.name === "string" && target.t.name !== "" ? target.t.name : "MMSI " + target.t.mmsi
                color: target.ink
                font.family: map.theme.font
                font.pixelSize: 11
                font.bold: !!target.t.danger
                style: Text.Outline
                styleColor: Qt.alpha(map.theme.background, 0.85)
            }
        }
    }

    // The waypoint: a ring with a cross.
    Item {
        z: 12
        visible: !!map.waypoint
        readonly property point at: map.waypoint ? map.px(map.waypoint.lat, map.waypoint.lon) : Qt.point(0, 0)
        x: at.x
        y: at.y
        Rectangle {
            x: -8; y: -8; width: 16; height: 16; radius: 8
            color: "transparent"
            border.color: map.theme.accent
            border.width: 2
        }
        Rectangle { x: -1; y: -12; width: 2; height: 24; color: map.theme.accent }
        Rectangle { x: -12; y: -1; width: 24; height: 2; color: map.theme.accent }
    }

    // Where the last query looked.
    Rectangle {
        z: 12
        visible: !!map.mark
        readonly property point at: map.mark ? map.px(map.mark.lat, map.mark.lon) : Qt.point(0, 0)
        x: at.x - 6
        y: at.y - 6
        width: 12; height: 12; radius: 6
        color: "transparent"
        border.color: map.theme.foreground
        border.width: 2
    }

    // Our boat: an arrowhead on the course made good, hollow when the fix
    // isn't current.
    Item {
        z: 13
        visible: !!map.boat
        x: map.boatAt.x
        y: map.boatAt.y
        rotation: map.lastCog
        Shape {
            preferredRendererType: Shape.CurveRenderer
            ShapePath {
                fillColor: map.boatLive ? map.theme.accent : "transparent"
                strokeColor: map.boatLive ? map.theme.background : map.theme.accent
                strokeWidth: map.boatLive ? 1.5 : 2
                joinStyle: ShapePath.MiterJoin
                startX: 0; startY: -14
                PathLine { x: 8; y: 10 }
                PathLine { x: 0; y: 5 }
                PathLine { x: -8; y: 10 }
                PathLine { x: 0; y: -14 }
            }
        }
    }

    // ---------------------------------------------------------------- input

    MouseArea {
        id: mouse
        anchors.fill: parent
        z: 20
        acceptedButtons: Qt.LeftButton | Qt.RightButton
        hoverEnabled: true
        cursorShape: dragging ? Qt.ClosedHandCursor : Qt.CrossCursor
        property point press: Qt.point(0, 0)
        property real startX: 0
        property real startY: 0
        property bool dragging: false
        onPressed: m => {
            press = Qt.point(m.x, m.y);
            startX = map.cx;
            startY = map.cy;
            dragging = false;
            map.forceActiveFocus();
        }
        onPositionChanged: m => {
            map.hover = Qt.point(m.x, m.y);
            map.hovering = true;
            if (!(pressedButtons & Qt.LeftButton)) return;
            var dx = m.x - press.x, dy = m.y - press.y;
            if (!dragging && Math.abs(dx) + Math.abs(dy) > 4) {
                dragging = true;
                map.panned();
            }
            if (dragging) map.setCenter(startX - dx / map.world, startY - dy / map.world);
        }
        onReleased: m => {
            if (dragging) { dragging = false; return; }
            var lat = Geo.lat(map.cy + (m.y - map.height / 2) / map.world);
            var lon = Geo.lon(map.cx + (m.x - map.width / 2) / map.world);
            map.pointed(lat, lon, m.x, m.y, m.button === Qt.RightButton ? "waypoint" : "query");
        }
        onExited: map.hovering = false
        onWheel: w => {
            var steps = w.angleDelta.y !== 0 ? w.angleDelta.y / 120 : w.pixelDelta.y / 50;
            map.zoomAt(map.zoom + steps * 0.5, w.x, w.y);
        }
    }
}
