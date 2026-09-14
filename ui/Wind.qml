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
            wind.socket.connected = false;
            return;
        }
        if (m.type === "state") {
            if (m.forecast !== null && typeof m.forecast === "object") wind.state = m;
        } else if (m.type === "field") {
            if (Array.isArray(m.points)) wind.field(m);
        } else if (m.type === "error") {
            wind.rejected(m);
        }
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
            onConnectedChanged: if (!connected) wind.state = null
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
