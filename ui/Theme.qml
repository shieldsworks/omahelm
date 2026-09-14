import QtQuick
import Quickshell
import Quickshell.Io

// The Omarchy theme: its colors.toml, and the shell's font size. Reloads
// when the theme changes, so the chrome follows a theme switch at once.
// The chart tiles follow on their own: the engine reads the same file.
QtObject {
    id: theme
    readonly property string dir: Quickshell.env("OMAHELM_THEME_DIR")
        || Quickshell.env("HOME") + "/.local/state/omarchy/current/theme"
    readonly property string shellConfig: Quickshell.env("HOME") + "/.config/omarchy/shell.toml"
    property var colors: ({})
    property var shellValues: ({})
    // Night Watch: red on black whatever the theme, to keep night vision.
    // Off at every start; this window's own, not the desktop's.
    property bool night: false
    readonly property var nightWatch: ({
        background: "#0c0404", foreground: "#e8503f", accent: "#ff3b2f",
        red: "#ff3b2f", yellow: "#ffa28a"
    })

    // `key = "value"` and `key = 12` lines, keyed section.key. Enough for
    // colors.toml and shell.toml; not a general TOML reader.
    function read(text) {
        var out = {}, section = "";
        var lines = String(text).split("\n");
        for (var i = 0; i < lines.length; i++) {
            var line = lines[i].trim();
            if (!line || line[0] === "#") continue;
            var head = line.match(/^\[([^\]]+)\]\s*(#.*)?$/);
            if (head) { section = head[1].trim() + "."; continue; }
            // A quoted value keeps its `#` (colors are "#rrggbb"); a bare
            // one ends at a comment.
            var kv = line.match(/^([\w.-]+)\s*=\s*(?:"([^"]*)"|'([^']*)'|([^#]*?))\s*(#.*)?$/);
            if (!kv) continue;
            var v = kv[2] !== undefined ? kv[2] : kv[3] !== undefined ? kv[3] : kv[4];
            if (kv[4] !== undefined && /^-?\d+(\.\d+)?$/.test(v)) v = Number(v);
            out[section + kv[1]] = v;
        }
        return out;
    }
    function color(key, fallback) {
        var v = night ? nightWatch[key] : colors[key];
        return typeof v === "string" && /^#[0-9a-fA-F]{6}$/.test(v) ? v : fallback;
    }

    readonly property color background: color("background", "#1a1b26")
    readonly property color foreground: color("foreground", "#a9b1d6")
    readonly property color accent: color("accent", "#7aa2f7")
    readonly property color red: color("red", "#f7768e")
    readonly property color yellow: color("yellow", "#e0af68")
    readonly property string font: "monospace"
    readonly property int baseSize: {
        var n = Number(shellValues["font.base-size"]);
        return n > 0 && n < 40 ? n : 12;
    }

    property FileView colorsFile: FileView {
        path: theme.dir + "/colors.toml"
        watchChanges: true
        printErrors: false
        onFileChanged: reload()
        onLoaded: theme.colors = theme.read(text())
        onLoadFailed: theme.colors = ({})
    }
    property FileView shellFile: FileView {
        path: theme.shellConfig
        watchChanges: true
        printErrors: false
        onFileChanged: reload()
        onLoaded: theme.shellValues = theme.read(text())
        onLoadFailed: theme.shellValues = ({})
    }
}
