import QtQuick
import Quickshell
import Quickshell.Io

// The connection to omawind, the wind engine, for the wind layer. The
// protocol is omawind's docs/protocol.md: newline-delimited JSON, version 1.
// omahelm doesn't start omawind: its bar widget does, or `omawind run`.
// Connected only while the layer is on.
QtObject {
    id: wind

    readonly property int version: 1
    readonly property string path: (Quickshell.env("XDG_RUNTIME_DIR") || "/tmp") + "/omawind/wind.sock"

    property bool wanted: false
    property var state: null
    // The last `stations`: the wind measured at NOAA's stations.
    property var stations: null
    property bool incompatible: false
    readonly property bool connected: socket !== null && socket.connected
    readonly property var forecast: state ? state.forecast : null

    signal field(var message)
    signal rejected(var message)

    function receive(line) {
        var m;
        try {
            m = JSON.parse(line);
        } catch (e) {
            return;
        }
        if (m === null || typeof m !== "object" || typeof m.v !== "number") return;
        if (m.v !== wind.version) {
            wind.incompatible = true;
            wind.state = null;
            wind.stations = null;
            wind.socket.connected = false;
            return;
        }
        if (m.type === "state") {
            if (m.forecast !== null && typeof m.forecast === "object") wind.state = m;
        } else if (m.type === "stations") {
            if (Array.isArray(m.stations)) wind.stations = wind.drawable(m);
        } else if (m.type === "field") {
            if (Array.isArray(m.points)) wind.field(m);
        } else if (m.type === "error") {
            wind.rejected(m);
        }
    }

    // The stations that can be drawn and named, so a broken engine can't
    // hang or break the chart: finite numbers in range, a direction left out
    // only in a calm, and text where the readout expects text.
    function drawable(m) {
        function num(v, lo, hi) { return typeof v === "number" && isFinite(v) && v >= lo && v <= hi; }
        var kept = m.stations.filter(s => s !== null && typeof s === "object"
            && typeof s.id === "string" && typeof s.time === "string"
            && (s.name === undefined || typeof s.name === "string")
            && num(s.lat, -90, 90) && num(s.lon, -180, 180) && num(s.speedKn, 0, 250)
            && (s.dirDeg === undefined ? s.speedKn === 0 : num(s.dirDeg, 0, 360))
            && (s.gustKn === undefined || num(s.gustKn, 0, 300)));
        return Object.assign({}, m, {stations: kept});
    }

    function send(message) {
        if (!wind.connected) return;
        wind.socket.write(JSON.stringify(message) + "\n");
        wind.socket.flush();
    }

    onWantedChanged: {
        if (wanted && socket === null) {
            // A new try: an engine that spoke another version may have
            // been updated since.
            incompatible = false;
            socket = socketFactory.createObject(wind);
        } else if (!wanted && socket !== null) {
            const previous = socket;
            socket = null;
            state = null;
            stations = null;
            previous.destroy();
        }
    }

    property var socket: null
    property Component socketFactory: Component {
        Socket {
            path: wind.path
            connected: true
            parser: SplitParser {
                onRead: data => wind.receive(data)
            }
            onConnectedChanged: if (!connected) { wind.state = null; wind.stations = null; }
        }
    }

    // A failed connect leaves Quickshell's socket allocated, and toggling
    // `connected` can't retry it, so each retry is a fresh Socket.
    property Timer reconnect: Timer {
        interval: 2000
        repeat: true
        running: wind.wanted && wind.socket !== null && !wind.socket.connected && !wind.incompatible
        onTriggered: {
            const previous = wind.socket;
            wind.socket = wind.socketFactory.createObject(wind);
            previous.destroy();
        }
    }
}
