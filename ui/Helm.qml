import QtQuick
import Quickshell
import Quickshell.Io

// The connection to the chart engine, `omahelm serve`. The protocol is
// docs/protocol.md: newline-delimited JSON, version 1. When the engine
// isn't running this starts it, detached, so it outlives the window.
QtObject {
    id: helm

    readonly property int version: 1
    readonly property string runtime: (Quickshell.env("XDG_RUNTIME_DIR") || "/tmp") + "/omahelm/"
    readonly property string path: runtime + "helm.sock"
    // The checkout or plugin directory: this file is <repo>/ui/Helm.qml.
    readonly property string repo: decodeURIComponent(String(Qt.resolvedUrl("..")).replace(/^file:\/\//, "")).replace(/\/$/, "")
    readonly property string binary: Quickshell.env("OMAHELM_BIN") || repo + "/target/release/omahelm"
    // The engine's stderr, so a failed start can be shown.
    readonly property string log: runtime + "engine.log"

    property var state: null
    property string error: ""
    property string lastLog: ""
    property bool incompatible: false
    // Unreachable for long enough that it's worth saying so.
    property bool waited: false
    property int attempts: 0
    readonly property bool connected: socket !== null && socket.connected

    signal tile(var message)
    signal features(var message)
    signal rejected(string message)
    // The logbook: `trips` is the index of days, `trip` one day's lines.
    signal index(var message)
    signal trip(var message)

    function receive(line) {
        var m;
        try {
            m = JSON.parse(line);
        } catch (e) {
            return;
        }
        if (m === null || typeof m !== "object" || typeof m.v !== "number") return;
        if (m.v !== helm.version) {
            helm.incompatible = true;
            helm.state = null;
            helm.error = "The chart engine speaks protocol version " + m.v + " and this window speaks " + helm.version + ". Update omahelm.";
            helm.socket.connected = false;
            return;
        }
        if (m.type === "state") {
            if (m.charts && m.tiles && typeof m.tiles.root === "string") {
                helm.state = m;
                helm.error = "";
            }
        } else if (m.type === "tile") {
            if (m.error !== undefined || helm.validPath(m.path)) helm.tile(m);
        } else if (m.type === "features") {
            helm.features(m);
        } else if (m.type === "trips") {
            if (Array.isArray(m.days)) helm.index(helm.listed(m));
        } else if (m.type === "trip") {
            helm.trip(helm.drawable(m));
        } else if (m.type === "error") {
            helm.rejected(String(m.message || ""));
        }
    }

    // Days the calendar can put a mark on: a date it can read and numbers
    // it can print.
    function listed(m) {
        var kept = m.days.filter(d => d !== null && typeof d === "object"
            && typeof d.date === "string" && /^\d{4}-\d{2}-\d{2}$/.test(d.date)
            && typeof d.distanceNm === "number" && isFinite(d.distanceNm)
            && typeof d.gapNm === "number" && isFinite(d.gapNm)
            && typeof d.passages === "number");
        return Object.assign({}, m, {days: kept});
    }

    // A day as the chart can draw it: lines of lat, lon pairs and nothing
    // else, so a broken engine can't put a stroke across the Pacific.
    function drawable(m) {
        function lines(v) {
            if (!Array.isArray(v)) return [];
            return v.filter(l => Array.isArray(l) && l.length >= 4 && l.length % 2 === 0
                && l.every((n, i) => typeof n === "number" && isFinite(n)
                    && (i % 2 ? n >= -180 && n <= 180 : n >= -90 && n <= 90)));
        }
        return Object.assign({}, m, {runs: lines(m.runs), gaps: lines(m.gaps)});
    }

    // `<z>/<x>/<y>@<scale>.png`, nothing else: a path can't climb out of
    // the tile directory.
    function validPath(p) {
        return typeof p === "string" && /^[0-9]+\/[0-9]+\/[0-9]+@[1-4]\.png$/.test(p);
    }

    function send(message) {
        if (!helm.connected) return;
        helm.socket.write(JSON.stringify(message) + "\n");
        helm.socket.flush();
    }

    // argv, never shell text built from paths. The engine keeps one copy
    // running through its lock file, so a second start is harmless.
    function start() {
        var script = 'mkdir -p -m 700 "$1" && b="$2"; [ -x "$b" ] || b=omahelm; exec "$b" serve 2>>"$3"';
        Quickshell.execDetached(["env", "-C", Quickshell.env("HOME") || "/", "sh", "-c", script,
                                 "omahelm-start", helm.runtime, helm.binary, helm.log]);
    }

    property var socket: socketFactory.createObject(helm)
    property Component socketFactory: Component {
        Socket {
            path: helm.path
            connected: true
            parser: SplitParser {
                onRead: data => helm.receive(data)
            }
            onConnectedChanged: {
                if (connected) {
                    helm.attempts = 0;
                    helm.waited = false;
                } else {
                    helm.state = null;
                }
            }
        }
    }

    // A failed connect leaves Quickshell's socket allocated, and toggling
    // `connected` can't retry it, so each retry is a fresh Socket. The
    // engine is started on the first failure and again every 20 s.
    property Timer reconnect: Timer {
        // Quickly at first, then gently: each failure is a line in the log.
        interval: helm.attempts < 10 ? 1000 : 3000
        repeat: true
        triggeredOnStart: false
        running: helm.socket !== null && !helm.socket.connected && !helm.incompatible
        onTriggered: {
            helm.attempts += 1;
            if (helm.attempts % 20 === 1) helm.start();
            if (helm.attempts >= 6) helm.waited = true;
            // The log may not have existed when it was first watched.
            helm.logFile.reload();
            const previous = helm.socket;
            helm.socket = helm.socketFactory.createObject(helm);
            previous.destroy();
        }
    }

    property FileView logFile: FileView {
        path: helm.log
        watchChanges: true
        printErrors: false
        onFileChanged: reload()
        onLoaded: {
            var lines = text().trim().split("\n");
            helm.lastLog = lines.length ? lines[lines.length - 1] : "";
        }
    }
}
