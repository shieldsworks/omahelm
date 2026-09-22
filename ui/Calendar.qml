import QtQuick

// The logbook's calendar: a month at a time, a mark under every day the
// boat went out, and the day under the cursor named below it. Opened with
// d, and the way you find a sail you took in June.
//
// It holds no sockets and reads no files. The window hands it the index
// the engine sent and it hands back the day you picked, so it can be
// driven from the keyboard, the mouse or the IPC hook alike.
Rectangle {
    id: calendar

    property var theme
    // The engine's index: one entry per day with a track, oldest first.
    property var days: []
    // The day the chart is showing, and the day under the cursor.
    property string selected: ""
    property string cursor: ""

    signal picked(string date)

    readonly property string today: Qt.formatDate(new Date(), "yyyy-MM-dd")
    readonly property var byDate: {
        var out = ({});
        for (var i = 0; i < days.length; i++) {
            if (days[i] && typeof days[i].date === "string") out[days[i].date] = days[i];
        }
        return out;
    }
    readonly property string month: cursor.length >= 7 ? cursor.slice(0, 7) : today.slice(0, 7)
    readonly property var here: byDate[cursor] || null

    function date(d) { return Qt.formatDate(d, "yyyy-MM-dd"); }
    function parse(s) {
        return s.length === 10
            ? new Date(Number(s.slice(0, 4)), Number(s.slice(5, 7)) - 1, Number(s.slice(8, 10)))
            : new Date();
    }
    // The cursor starts on the day being shown, else the last day sailed,
    // else today: never on an empty month the user has to climb out of.
    function open() {
        if (cursor === "") cursor = selected || (days.length ? days[days.length - 1].date : today);
    }
    function step(byDays) {
        var d = parse(cursor);
        d.setDate(d.getDate() + byDays);
        if (d.getFullYear() >= 1990 && d.getFullYear() <= 2100) cursor = date(d);
    }
    // A month step keeps the day of the month where it can: the 31st of
    // March goes back to the 28th of February, not on to the 3rd of March.
    function stepMonth(by) {
        var d = parse(cursor);
        var day = d.getDate();
        var first = new Date(d.getFullYear(), d.getMonth() + by, 1);
        var last = new Date(first.getFullYear(), first.getMonth() + 1, 0).getDate();
        first.setDate(Math.min(day, last));
        if (first.getFullYear() >= 1990 && first.getFullYear() <= 2100) cursor = date(first);
    }
    // The next day with a track, in either direction: the way past a
    // fortnight ashore without pressing an arrow fourteen times.
    function stepSailed(by) {
        for (var i = 0; i < days.length; i++) {
            var at = by > 0 ? days[i].date : days[days.length - 1 - i].date;
            if (by > 0 ? at > cursor : at < cursor) { cursor = at; return; }
        }
    }
    function pick() {
        if (byDate[cursor]) calendar.picked(cursor);
    }

    // Six weeks from the Sunday on or before the first of the month, so
    // the grid never changes height as the months turn.
    readonly property var cells: {
        var y = Number(month.slice(0, 4)), m = Number(month.slice(5, 7)) - 1;
        var start = new Date(y, m, 1);
        start.setDate(1 - start.getDay());
        var out = [];
        for (var i = 0; i < 42; i++) {
            var d = new Date(start.getFullYear(), start.getMonth(), start.getDate() + i);
            out.push({at: date(d), day: d.getDate(), inMonth: d.getMonth() === m});
        }
        return out;
    }

    // Wide enough for the day's summary under the grid, which is the
    // line that decides whether a day is the one you were looking for.
    readonly property int cellSize: Math.floor((width - 32) / 7)
    width: Math.max(theme.baseSize * 26, 260)
    height: column.implicitHeight + 24
    color: theme.background
    border.width: 1
    border.color: theme.accent

    // Clicks inside the calendar never reach the chart under it.
    MouseArea { anchors.fill: parent }

    component Label: Text {
        textFormat: Text.PlainText
        color: calendar.theme.foreground
        font.family: calendar.theme.font
        font.pixelSize: calendar.theme.baseSize
        elide: Text.ElideRight
    }

    Column {
        id: column
        x: 16
        y: 12
        width: parent.width - 32
        spacing: 6

        // The month, with a step either side.
        Item {
            width: parent.width
            height: calendar.theme.baseSize + 10
            Label {
                anchors { left: parent.left; verticalCenter: parent.verticalCenter }
                text: "‹"
                color: calendar.theme.accent
                MouseArea { anchors.fill: parent; anchors.margins: -8; onClicked: calendar.stepMonth(-1) }
            }
            Label {
                anchors.centerIn: parent
                text: Qt.formatDate(calendar.parse(calendar.month + "-01"), "MMMM yyyy")
                font.bold: true
                color: calendar.theme.accent
            }
            Label {
                anchors { right: parent.right; verticalCenter: parent.verticalCenter }
                text: "›"
                color: calendar.theme.accent
                MouseArea { anchors.fill: parent; anchors.margins: -8; onClicked: calendar.stepMonth(1) }
            }
        }

        Row {
            Repeater {
                model: ["S", "M", "T", "W", "T", "F", "S"]
                Label {
                    required property var modelData
                    width: calendar.cellSize
                    horizontalAlignment: Text.AlignHCenter
                    text: modelData
                    color: Qt.alpha(calendar.theme.foreground, 0.5)
                    font.pixelSize: calendar.theme.baseSize - 2
                }
            }
        }

        Grid {
            columns: 7
            Repeater {
                model: calendar.cells
                Item {
                    id: cell
                    required property var modelData
                    readonly property var day: calendar.byDate[modelData.at] || null
                    width: calendar.cellSize
                    height: calendar.cellSize
                    // The day under the cursor.
                    Rectangle {
                        anchors.fill: parent
                        anchors.margins: 1
                        visible: calendar.cursor === cell.modelData.at
                        color: cell.day ? calendar.theme.accent : Qt.alpha(calendar.theme.foreground, 0.18)
                    }
                    // Today, wherever the cursor is.
                    Rectangle {
                        anchors.fill: parent
                        anchors.margins: 1
                        visible: calendar.today === cell.modelData.at
                        color: "transparent"
                        border.width: 1
                        border.color: Qt.alpha(calendar.theme.foreground, 0.5)
                    }
                    Label {
                        anchors.horizontalCenter: parent.horizontalCenter
                        y: (parent.height - height) / 2 - 3
                        text: cell.modelData.day
                        font.bold: !!cell.day
                        color: calendar.cursor === cell.modelData.at && cell.day ? calendar.theme.background
                            : !cell.modelData.inMonth ? Qt.alpha(calendar.theme.foreground, 0.3)
                            : cell.day ? calendar.theme.foreground : Qt.alpha(calendar.theme.foreground, 0.55)
                    }
                    // A day with a track is marked, and the one on the
                    // chart is marked solid.
                    Rectangle {
                        visible: !!cell.day
                        anchors.horizontalCenter: parent.horizontalCenter
                        y: parent.height - 8
                        width: 5
                        height: 5
                        radius: 2.5
                        color: calendar.selected === cell.modelData.at ? calendar.theme.accent
                            : calendar.cursor === cell.modelData.at ? calendar.theme.background
                            : Qt.alpha(calendar.theme.accent, 0.7)
                        border.width: calendar.selected === cell.modelData.at ? 0 : 1
                        border.color: calendar.theme.accent
                    }
                    MouseArea {
                        anchors.fill: parent
                        onClicked: {
                            calendar.cursor = cell.modelData.at;
                            calendar.pick();
                        }
                    }
                }
            }
        }

        Rectangle { width: parent.width; height: 1; color: Qt.alpha(calendar.theme.foreground, 0.18) }

        // The day under the cursor, in the words the status line uses.
        Label {
            width: parent.width
            text: Qt.formatDate(calendar.parse(calendar.cursor), "ddd d MMM yyyy")
            font.bold: true
        }
        Label {
            width: parent.width
            text: {
                var d = calendar.here;
                if (!d) return "Nothing logged";
                var out = d.passages + (d.passages === 1 ? " passage" : " passages")
                    + "   " + d.distanceNm.toFixed(1) + " nm";
                if (d.gapNm > 0.05) out += "   " + d.gapNm.toFixed(1) + " inferred";
                return out;
            }
            wrapMode: Text.Wrap
            color: calendar.here ? calendar.theme.foreground : Qt.alpha(calendar.theme.foreground, 0.6)
        }
        Label {
            width: parent.width
            visible: !!calendar.here && typeof calendar.here.from === "string"
            text: {
                var d = calendar.here;
                if (!d || typeof d.from !== "string") return "";
                var from = new Date(d.from), to = new Date(d.to);
                var hours = Math.floor((d.seconds || 0) / 3600), minutes = Math.round(((d.seconds || 0) % 3600) / 60);
                return Qt.formatDateTime(from, "HH:mm") + "–" + Qt.formatDateTime(to, "HH:mm")
                    + "   " + hours + " h " + minutes + " min";
            }
            color: Qt.alpha(calendar.theme.foreground, 0.7)
            font.pixelSize: calendar.theme.baseSize - 1
        }
        Item { width: 1; height: 2 }
        Repeater {
            model: ["enter show   [ ] month   { } sail",
                    "x all trips   t today   esc close"]
            Label {
                required property var modelData
                width: column.width
                text: modelData
                color: Qt.alpha(calendar.theme.foreground, 0.55)
                font.pixelSize: calendar.theme.baseSize - 2
            }
        }
    }
}
