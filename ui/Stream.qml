import QtQuick
import Quickshell
import Quickshell.Io

// The connection to omatide, the tide engine, for the stream layer. The
// protocol is omatide's docs/protocol.md: newline-delimited JSON,
// version 1. omahelm doesn't start omatide: its bar widget does, or
// `omatide run`. Connected only while the layer is on.
//
// The same shape as Wind.qml, because it is the same job. What differs is
// what comes back: omawind predicts a grid, omatide predicts stations, so
// there is no field to interpolate, only arrows where NOAA measured.
QtObject {
    id: stream

    readonly property int version: 1
    readonly property string path: (Quickshell.env("XDG_RUNTIME_DIR") || "/tmp") + "/omatide/tide.sock"

    property bool wanted: false
    property var state: null
    property bool incompatible: false
    readonly property bool connected: socket !== null && socket.connected
    // How many stations omatide has, so the chart can say why it is empty.
    readonly property int stations: state && state.stations
        && typeof state.stations.current === "number" ? state.stations.current : -1

    signal streams(var message)
    signal curve(var message)
    signal rejected(var message)

    function num(v, lo, hi) { return typeof v === "number" && isFinite(v) && v >= lo && v <= hi; }

    function receive(line) {
        var m;
        try {
            m = JSON.parse(line);
        } catch (e) {
            return;
        }
        if (m === null || typeof m !== "object" || typeof m.v !== "number") return;
        // Lines already buffered after another version's are dropped too.
        if (stream.incompatible) return;
        if (m.v !== stream.version) {
            stream.incompatible = true;
            stream.state = null;
            stream.socket.connected = false;
            return;
        }
        if (m.type === "state") {
            if (m.here !== null && typeof m.here === "object") stream.state = m;
        } else if (m.type === "streams") {
            if (Array.isArray(m.streams)) stream.streams(stream.drawable(m));
        } else if (m.type === "curve") {
            if (Array.isArray(m.values)) stream.curve(stream.plottable(m));
        } else if (m.type === "error") {
            stream.rejected(m);
        }
    }

    // The arrows that can be drawn and named, so a broken engine can't
    // hang or break the chart: finite numbers in range, a direction left
    // out only at slack, and text where the readout expects text.
    function drawable(m) {
        var kept = m.streams.filter(s => s !== null && typeof s === "object"
            && typeof s.station === "string"
            && (s.name === undefined || typeof s.name === "string")
            && num(s.lat, -90, 90) && num(s.lon, -180, 180)
            && num(s.knots, 0, 30)
            && (s.way === "flood" || s.way === "ebb" || s.way === "slack")
            && (s.setDeg === undefined || num(s.setDeg, 0, 360))
            && (s.depthM === undefined || num(s.depthM, 0, 2000)));
        return Object.assign({}, m, {streams: kept});
    }

    // A curve as the time bar can plot it: a start it can read, a step
    // that moves, and nothing but finite knots in it. A stream's values
    // are signed — positive on the flood.
    function plottable(m) {
        var start = typeof m.start === "string" ? Date.parse(m.start) : NaN;
        var step = num(m.stepSeconds, 1, 86400) ? m.stepSeconds : 0;
        if (isNaN(start) || !step) return Object.assign({}, m, {values: [], turns: []});
        var values = m.values.filter(v => num(v, -30, 30));
        // One bad number would put every later one at the wrong time, so
        // a curve with any is dropped rather than drawn askew.
        if (values.length !== m.values.length) values = [];
        var turns = Array.isArray(m.turns) ? m.turns.filter(t => t !== null && typeof t === "object"
            && typeof t.turn === "string" && typeof t.time === "string"
            && !isNaN(Date.parse(t.time))) : [];
        return Object.assign({}, m, {values: values, turns: turns});
    }

    function send(message) {
        if (!stream.connected) return;
        stream.socket.write(JSON.stringify(message) + "\n");
        stream.socket.flush();
    }

    onWantedChanged: {
        if (wanted && socket === null) {
            // A new try: an engine that spoke another version may have
            // been updated since.
            incompatible = false;
            socket = socketFactory.createObject(stream);
        } else if (!wanted && socket !== null) {
            const previous = socket;
            socket = null;
            state = null;
            previous.destroy();
        }
    }

    property var socket: null
    property Component socketFactory: Component {
        Socket {
            path: stream.path
            connected: true
            parser: SplitParser {
                onRead: data => stream.receive(data)
            }
            onConnectedChanged: if (!connected) stream.state = null;
        }
    }

    // A failed connect leaves Quickshell's socket allocated, and toggling
    // `connected` can't retry it, so each retry is a fresh Socket.
    property Timer reconnect: Timer {
        interval: 2000
        repeat: true
        running: stream.wanted && stream.socket !== null && !stream.socket.connected
                 && !stream.incompatible
        onTriggered: {
            const previous = stream.socket;
            stream.socket = stream.socketFactory.createObject(stream);
            previous.destroy();
        }
    }
}
