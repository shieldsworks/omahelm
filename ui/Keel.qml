import QtQuick
import Quickshell
import Quickshell.Io

// The connection to omakeel, the boat's data hub: our fix and the AIS
// targets. The protocol is omakeel's docs/protocol.md, version 1. Helm
// only listens.
QtObject {
    id: keel

    readonly property int version: 1
    readonly property string path: (Quickshell.env("XDG_RUNTIME_DIR") || "/tmp") + "/omakeel/keel.sock"

    property var fix: null
    property var targets: []
    property bool incompatible: false
    readonly property bool connected: socket !== null && socket.connected
    // When the last state and targets arrived, and a clock to age them by.
    property real stateAt: 0
    // The fix's age when that state arrived.
    property real stateAge: 0
    property real targetsAt: 0
    property real now: Date.now()

    function receive(line) {
        let message;
        try {
            message = JSON.parse(line);
        } catch (e) {
            return;
        }
        if (message === null || typeof message !== "object" || typeof message.v !== "number") return;
        if (message.v !== keel.version) {
            keel.incompatible = true;
            keel.socket.connected = false;
            return;
        }
        if (message.type === "state") {
            keel.fix = message.fix !== null && typeof message.fix === "object" ? message.fix : null;
            keel.stateAt = Date.now();
            keel.stateAge = keel.fix && typeof keel.fix.ageSeconds === "number" ? keel.fix.ageSeconds : 0;
        } else if (message.type === "targets" && Array.isArray(message.targets)) {
            keel.targets = message.targets.filter(t => t !== null && typeof t === "object" && typeof t.mmsi === "number");
            keel.targetsAt = Date.now();
        }
    }

    // A vessel's name, or its MMSI until a name arrives.
    function called(t) {
        return typeof t.name === "string" && t.name !== "" ? t.name : "MMSI " + t.mmsi;
    }

    property var socket: socketFactory.createObject(keel)
    property Component socketFactory: Component {
        Socket {
            path: keel.path
            connected: true
            parser: SplitParser {
                onRead: data => keel.receive(data)
            }
            onConnectedChanged: {
                if (!connected) {
                    keel.fix = null;
                    keel.targets = [];
                }
            }
        }
    }

    // omakeel re-sends its state every second. Silence from a hub that is
    // hung but still connected must not leave an old fix looking live.
    property Timer clock: Timer {
        interval: 1000
        repeat: true
        running: true
        onTriggered: {
            keel.now = Date.now();
            const quiet = keel.now - keel.stateAt;
            // Stale, and its age still counting, until omakeel speaks again.
            if (keel.fix && quiet > 5000) {
                const age = keel.stateAge + Math.round(quiet / 1000);
                if (keel.fix.status === "ok" || keel.fix.ageSeconds !== age) {
                    const f = Object.assign({}, keel.fix);
                    if (f.status === "ok") f.status = "stale";
                    f.ageSeconds = age;
                    keel.fix = f;
                }
            }
        }
    }

    // Each retry is a fresh Socket: a failed one can't be reconnected.
    property Timer reconnect: Timer {
        interval: 2000
        repeat: true
        running: keel.socket !== null && !keel.socket.connected && !keel.incompatible
        onTriggered: {
            const previous = keel.socket;
            keel.socket = keel.socketFactory.createObject(keel);
            previous.destroy();
        }
    }
}
