import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import se.cptjanst.babel

// The text view: the document as rows of segments, the original beside the
// translation (as in a CAT tool), with Claude's translation of the row being
// worked on in a panel. It edits the same translation as the pages, through
// the same editor: typing, formatting, undo, done marks and the right-click
// actions are the page view's.
Item {
    id: view

    required property var page
    // Every row, as the editor gives them; and where each is in it.
    property var all: []
    property var at: ({})
    // Where each shown row is in the list.
    property var shownAt: ({})
    property string activePid: ""
    property var claudeSpans: null
    // While a row's own typing is answered: the row keeps its text and cursor.
    property bool typing: false
    property string search: ""
    property bool onlyOpen: false

    readonly property color chipBack: Qt.alpha(Kirigami.Theme.textColor, 0.12)
    readonly property color chipText: Kirigami.Theme.disabledTextColor
    readonly property color linkColor: Kirigami.Theme.linkColor

    function esc(t) {
        return t.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/\n/g, "<br>");
    }

    // Spans as rich text: bold, italic, underline, links; chips shaded.
    function html(spans) {
        let h = "";
        for (const s of spans || []) {
            if (s.chip !== undefined) {
                // Numbered chips shaded; what isn't numbered is there but not seen.
                h += s.chip ? '<span style="background-color:' + chipBack + '; color:' + chipText + ';">' + esc(s.t) + "</span>" : esc(s.t);
                continue;
            }
            let t = esc(s.t);
            if (s.b)
                t = "<b>" + t + "</b>";
            if (s.i)
                t = "<i>" + t + "</i>";
            if (s.u)
                t = "<u>" + t + "</u>";
            if (s.l)
                t = '<span style="color:' + linkColor + '; text-decoration: underline;">' + t + "</span>";
            h += t;
        }
        return '<p style="white-space: pre-wrap; margin: 0;">' + h + "</p>";
    }

    // Plain text as a text field gives it back (line breaks and no-break spaces as Qt has them).
    function plain(spans) {
        return norm((spans || []).map(s => s.t).join(""));
    }

    function norm(t) {
        return t.replace(/[\u2028\u2029]/g, "\n").replace(/\u00a0/g, " ");
    }

    function textOf(spans) {
        return (spans || []).filter(s => s.chip === undefined).map(s => s.t).join("");
    }

    function matches(r) {
        if (onlyOpen && r.done)
            return false;
        if (!search)
            return true;
        const q = search.toLowerCase();
        return textOf(r.src).toLowerCase().includes(q) || textOf(r.tgt).toLowerCase().includes(q);
    }

    function entry(r) {
        return {
            pid: r.pid,
            n: r.n,
            part: r.part || "",
            srcHtml: html(r.src),
            tgtHtml: html(r.tgt),
            tgtPlain: plain(r.tgt),
            done: !!r.done,
            edited: !!r.edited,
            claude: !!r.claude,
            editable: !!r.editable,
        };
    }

    function load() {
        all = JSON.parse(Backend.nativeRows()) || [];
        reindex();
        rebuild();
        activePid = "";
        if (rows.count)
            Qt.callLater(() => focusRow(rows.get(0).pid, 0, 0));
    }

    function reindex() {
        const m = {};
        all.forEach((r, i) => m[r.pid] = i);
        at = m;
    }

    function rebuild() {
        rows.clear();
        const m = {};
        for (const r of all) {
            if (!matches(r))
                continue;
            m[r.pid] = rows.count;
            rows.append(entry(r));
        }
        shownAt = m;
    }

    // After any answer of the editor: rows changed are shown again, the caret goes where it is.
    function afterAnswer(r) {
        const ch = JSON.parse(Backend.nativeRowChanges());
        if (ch && ch.all) {
            all = ch.rows;
            reindex();
            rebuild();
            claudeSpans = activePid ? JSON.parse(Backend.nativeClaudeSpans(activePid)) : null;
        } else if (ch) {
            for (const row of ch.rows) {
                if (at[row.pid] !== undefined)
                    all[at[row.pid]] = row;
                const i = shownAt[row.pid];
                if (i === undefined)
                    continue;
                rows.set(i, entry(row));
                // The same text as before (an edit refused): the field may still hold what was typed.
                const item = list.itemAtIndex(i);
                if (item)
                    item.reconcile();
            }
        }
        const tc = r.textCaret;
        if (tc && !typing)
            focusRow(tc.pid, tc.anchor, tc.at);
        if (tc)
            setActive(tc.pid);
        typing = false;
    }

    function setActive(pid) {
        if (pid === activePid)
            return;
        activePid = pid;
        claudeSpans = JSON.parse(Backend.nativeClaudeSpans(pid));
    }

    // Puts the cursor (and selection: anchor to at) in a row's translation.
    function focusRow(pid, anchor, atPos) {
        const i = shownAt[pid];
        if (i === undefined)
            return;
        list.positionViewAtIndex(i, ListView.Contain);
        const item = list.itemAtIndex(i);
        if (item)
            item.place(anchor, atPos);
    }

    // The next (dir 1) or previous (-1) row, at its start or end.
    function move(fromPid, dir) {
        const i = shownAt[fromPid];
        if (i === undefined)
            return;
        const j = i + dir;
        if (j < 0 || j >= rows.count)
            return;
        const p = rows.get(j).pid;
        const end = dir < 0 ? rows.get(j).tgtPlain.length : 0;
        focusRow(p, end, end);
    }

    // The editor's selection is the row's: then keys and menu actions act there.
    function sync(pid, edit) {
        const anchor = edit.cursorPosition === edit.selectionStart ? edit.selectionEnd : edit.selectionStart;
        page.answer(Backend.nativeTextSelect(pid, anchor, edit.cursorPosition), false);
    }

    // For the UI tests: a row's rich text as the editor has it.
    function rowHtml(pid) {
        const r = all[at[pid]];
        return r ? html(r.tgt) : "";
    }

    // For the UI tests: the active row's text as its field holds it.
    function activeText() {
        const i = shownAt[activePid];
        const item = i === undefined ? null : list.itemAtIndex(i);
        return item ? item.text() : "";
    }

    ListModel {
        id: rows
    }

    Timer {
        id: refilter
        interval: 200
        onTriggered: view.rebuild()
    }

    ColumnLayout {
        anchors.fill: parent
        spacing: 0

        // Search and filter.
        QQC2.ToolBar {
            Layout.fillWidth: true
            RowLayout {
                anchors.fill: parent
                Kirigami.SearchField {
                    Layout.preferredWidth: Kirigami.Units.gridUnit * 18
                    placeholderText: "Search the original and the translation"
                    onTextChanged: {
                        view.search = text;
                        refilter.restart();
                    }
                }
                QQC2.CheckBox {
                    text: "Only rows not done"
                    checked: view.onlyOpen
                    onToggled: {
                        view.onlyOpen = checked;
                        view.rebuild();
                    }
                }
                Item {
                    Layout.fillWidth: true
                }
                QQC2.Label {
                    color: Kirigami.Theme.disabledTextColor
                    text: rows.count === view.all.length ? view.all.length + " segments" : rows.count + " of " + view.all.length + " segments"
                }
            }
        }

        RowLayout {
            Layout.fillWidth: true
            Layout.fillHeight: true
            spacing: 0

            ListView {
                id: list
                Layout.fillWidth: true
                Layout.fillHeight: true
                clip: true
                model: rows
                reuseItems: false
                cacheBuffer: 2000
                boundsBehavior: Flickable.StopAtBounds
                QQC2.ScrollBar.vertical: QQC2.ScrollBar {}

                delegate: Rectangle {
                    id: row
                    required property int index
                    required property string pid
                    required property int n
                    required property string part
                    required property string srcHtml
                    required property string tgtHtml
                    required property string tgtPlain
                    required property bool done
                    required property bool edited
                    required property bool claude
                    required property bool editable

                    readonly property bool active: view.activePid === pid
                    width: ListView.view.width
                    implicitHeight: rowBox.implicitHeight + Kirigami.Units.largeSpacing * 2
                    color: active ? Qt.alpha(Kirigami.Theme.highlightColor, 0.10) : (index % 2 ? Kirigami.Theme.alternateBackgroundColor : Kirigami.Theme.backgroundColor)

                    function place(anchor, atPos) {
                        tgt.forceActiveFocus();
                        tgt.cursorPosition = Math.min(anchor, tgt.length);
                        tgt.moveCursorSelection(Math.min(atPos, tgt.length), TextEdit.SelectCharacters);
                    }

                    function reconcile() {
                        if (tgt.known !== tgtPlain)
                            tgt.show();
                    }

                    function text() {
                        return view.norm(tgt.getText(0, tgt.length));
                    }

                    // The editor's text came: shown unless it is what the field already holds
                    // after its own typing (then the cursor and the input method stay as they are).
                    onTgtHtmlChanged: {
                        if (tgt.activeFocus && view.typing && tgt.known === tgtPlain)
                            return;
                        tgt.show();
                    }

                    Rectangle {
                        visible: row.active
                        width: 3
                        height: parent.height
                        color: Kirigami.Theme.highlightColor
                    }

                    RowLayout {
                        id: rowBox
                        anchors.left: parent.left
                        anchors.right: parent.right
                        anchors.top: parent.top
                        anchors.margins: Kirigami.Units.largeSpacing
                        anchors.leftMargin: Kirigami.Units.largeSpacing + 3
                        spacing: Kirigami.Units.largeSpacing

                        ColumnLayout {
                            Layout.preferredWidth: Kirigami.Units.gridUnit * 2.5
                            Layout.alignment: Qt.AlignTop
                            spacing: 0
                            QQC2.Label {
                                text: row.n
                                color: Kirigami.Theme.disabledTextColor
                                font: Kirigami.Theme.smallFont
                            }
                            QQC2.Label {
                                visible: row.part !== ""
                                text: row.part
                                color: Kirigami.Theme.disabledTextColor
                                font: Kirigami.Theme.smallFont
                            }
                            Kirigami.Icon {
                                visible: row.done || row.edited
                                implicitWidth: Kirigami.Units.iconSizes.small
                                implicitHeight: Kirigami.Units.iconSizes.small
                                source: row.done ? "checkmark" : "document-edit"
                                color: row.done ? Kirigami.Theme.positiveTextColor : Kirigami.Theme.linkColor
                            }
                        }

                        // The original: to read, select and copy.
                        TextEdit {
                            id: src
                            Layout.fillWidth: true
                            Layout.preferredWidth: 1
                            Layout.alignment: Qt.AlignTop
                            readOnly: true
                            selectByMouse: true
                            textFormat: TextEdit.RichText
                            wrapMode: TextEdit.Wrap
                            text: row.srcHtml
                            color: Kirigami.Theme.textColor
                            selectionColor: Kirigami.Theme.highlightColor
                            selectedTextColor: Kirigami.Theme.highlightedTextColor
                            font: Kirigami.Theme.defaultFont
                            TapHandler {
                                acceptedButtons: Qt.RightButton
                                onTapped: srcMenu.show(row.pid, src.selectedText)
                            }
                        }

                        Rectangle {
                            Layout.fillHeight: true
                            implicitWidth: 1
                            color: Kirigami.Theme.disabledTextColor
                            opacity: 0.3
                        }

                        // The translation.
                        TextEdit {
                            id: tgt
                            Layout.fillWidth: true
                            Layout.preferredWidth: 1
                            Layout.alignment: Qt.AlignTop
                            readOnly: !row.editable
                            selectByMouse: true
                            persistentSelection: true
                            textFormat: TextEdit.RichText
                            wrapMode: TextEdit.Wrap
                            color: Kirigami.Theme.textColor
                            selectionColor: Kirigami.Theme.highlightColor
                            selectedTextColor: Kirigami.Theme.highlightedTextColor
                            font: Kirigami.Theme.defaultFont

                            // What the field holds, as the editor has it (before typing is answered).
                            property string known: ""
                            property bool updating: false

                            function show() {
                                const anchor = cursorPosition === selectionStart ? selectionEnd : selectionStart;
                                const pos = cursorPosition;
                                updating = true;
                                text = row.tgtHtml;
                                updating = false;
                                known = view.norm(getText(0, length));
                                if (activeFocus) {
                                    cursorPosition = Math.min(anchor, length);
                                    moveCursorSelection(Math.min(pos, length), TextEdit.SelectCharacters);
                                }
                            }

                            Component.onCompleted: show()

                            onTextChanged: {
                                if (updating)
                                    return;
                                const now = view.norm(getText(0, length));
                                const was = known;
                                // Only typing is an edit, and typing is where the keyboard is
                                // (a field shows its first text without focus).
                                if (!activeFocus) {
                                    known = now;
                                    return;
                                }
                                if (now === was)
                                    return;
                                // What changed: the part between what stayed at the start and at the end.
                                let a = 0;
                                while (a < was.length && a < now.length && was[a] === now[a])
                                    a++;
                                let b = 0;
                                while (b < was.length - a && b < now.length - a && was[was.length - 1 - b] === now[now.length - 1 - b])
                                    b++;
                                known = now;
                                view.typing = true;
                                view.page.answer(Backend.nativeTextEdit(row.pid, a, was.length - b, now.substring(a, now.length - b)), false);
                            }

                            onActiveFocusChanged: {
                                if (activeFocus) {
                                    view.setActive(row.pid);
                                    view.sync(row.pid, tgt);
                                } else {
                                    // Leaving the row: shown as the editor has it (formatting of typed text).
                                    show();
                                }
                            }

                            Keys.onPressed: event => {
                                const ctrl = (event.modifiers & Qt.ControlModifier) !== 0;
                                const shift = (event.modifiers & Qt.ShiftModifier) !== 0;
                                const enter = event.key === Qt.Key_Return || event.key === Qt.Key_Enter;
                                if (enter && ctrl) {
                                    view.done(row.pid, tgt);
                                    event.accepted = true;
                                } else if (enter && !shift) {
                                    view.move(row.pid, 1);
                                    event.accepted = true;
                                } else if (ctrl && (event.key === Qt.Key_B || event.key === Qt.Key_I || event.key === Qt.Key_U || event.key === Qt.Key_Z || event.key === Qt.Key_Y)) {
                                    view.sync(row.pid, tgt);
                                    view.page.key(event.key, "", event.modifiers);
                                    event.accepted = true;
                                } else if (ctrl && event.key === Qt.Key_K) {
                                    view.sync(row.pid, tgt);
                                    view.page.pasteLink();
                                    event.accepted = true;
                                } else if (ctrl && event.key === Qt.Key_V) {
                                    view.sync(row.pid, tgt);
                                    view.page.paste();
                                    event.accepted = true;
                                } else if (ctrl && (event.key === Qt.Key_X || event.key === Qt.Key_C)) {
                                    view.sync(row.pid, tgt);
                                    view.page.copy(event.key === Qt.Key_X);
                                    event.accepted = true;
                                } else if (ctrl && event.key === Qt.Key_1) {
                                    view.insertClaude(row.pid, tgt);
                                    event.accepted = true;
                                } else if (event.key === Qt.Key_Up && !shift && positionToRectangle(cursorPosition).y < 2) {
                                    view.move(row.pid, -1);
                                    event.accepted = true;
                                } else if (event.key === Qt.Key_Down && !shift && positionToRectangle(cursorPosition).y + positionToRectangle(cursorPosition).height > contentHeight - 2) {
                                    view.move(row.pid, 1);
                                    event.accepted = true;
                                }
                            }

                            TapHandler {
                                acceptedButtons: Qt.RightButton
                                onTapped: eventPoint => {
                                    if (tgt.selectionStart === tgt.selectionEnd)
                                        tgt.cursorPosition = tgt.positionAt(eventPoint.position.x, eventPoint.position.y);
                                    tgt.forceActiveFocus();
                                    view.sync(row.pid, tgt);
                                    view.page.openContextMenu();
                                }
                            }
                        }
                    }
                }
            }

            Kirigami.Separator {
                Layout.fillHeight: true
            }

            // The row being worked on: Claude's translation of it, and what to do with it.
            QQC2.ScrollView {
                Layout.preferredWidth: Kirigami.Units.gridUnit * 18
                Layout.fillHeight: true
                ColumnLayout {
                    width: Kirigami.Units.gridUnit * 18 - Kirigami.Units.largeSpacing * 2
                    x: Kirigami.Units.largeSpacing
                    spacing: Kirigami.Units.largeSpacing

                    Kirigami.Heading {
                        Layout.topMargin: Kirigami.Units.largeSpacing
                        level: 4
                        text: view.activePid && view.at[view.activePid] !== undefined ? "Segment " + view.all[view.at[view.activePid]].n : "Segment"
                    }

                    QQC2.Label {
                        Layout.fillWidth: true
                        font.bold: true
                        text: "Claude"
                    }
                    TextEdit {
                        Layout.fillWidth: true
                        visible: !!view.claudeSpans
                        readOnly: true
                        selectByMouse: true
                        textFormat: TextEdit.RichText
                        wrapMode: TextEdit.Wrap
                        color: Kirigami.Theme.textColor
                        font: Kirigami.Theme.defaultFont
                        text: view.claudeSpans ? view.html(view.claudeSpans) : ""
                    }
                    QQC2.Label {
                        Layout.fillWidth: true
                        visible: !view.claudeSpans
                        wrapMode: Text.Wrap
                        color: Kirigami.Theme.disabledTextColor
                        text: view.page.claude ? "Claude's translation has no segment for this row." : "No translation by Claude is attached to this document."
                    }
                    QQC2.Button {
                        visible: !!view.claudeSpans
                        icon.name: "document-import"
                        text: "Insert Claude's Translation"
                        QQC2.ToolTip.text: "Ctrl+1"
                        QQC2.ToolTip.visible: hovered
                        onClicked: view.insertClaude(view.activePid, null)
                    }
                    QQC2.Button {
                        visible: !view.page.claude
                        icon.name: "document-import"
                        text: "Attach Claude's Translation…"
                        onClicked: view.page.attachClaude()
                    }

                    Kirigami.Separator {
                        Layout.fillWidth: true
                    }

                    QQC2.Button {
                        icon.name: "checkmark"
                        text: view.page.paragraph && view.page.paragraph.done ? "Unmark as Done" : "Mark as Done"
                        enabled: view.activePid !== ""
                        QQC2.ToolTip.text: "Ctrl+Enter: and go to the next segment"
                        QQC2.ToolTip.visible: hovered
                        onClicked: view.done(view.activePid, null)
                    }
                    QQC2.Button {
                        icon.name: "edit-reset"
                        text: "Revert to the Original"
                        enabled: !!(view.page.paragraph && view.page.paragraph.edited)
                        onClicked: view.page.answer(Backend.nativeRevert(), false)
                    }
                }
            }
        }
    }

    // Marks a row done (or not); marked, on to the next row of the grid (the
    // pages' Ctrl+Enter goes to what is below on the page, here that's the next row).
    function done(pid, edit) {
        if (!pid)
            return;
        if (edit)
            sync(pid, edit);
        else
            page.answer(Backend.nativeTextSelect(pid, 0, 0), false);
        page.key(Qt.Key_Return, "", Qt.ControlModifier);
        const r = all[at[pid]];
        if (r && r.done)
            move(pid, 1);
    }

    function insertClaude(pid, edit) {
        if (!pid)
            return;
        if (edit)
            sync(pid, edit);
        else
            page.answer(Backend.nativeTextSelect(pid, 0, 0), false);
        page.answer(Backend.nativeInsertClaude(), false);
    }

    // The original's menu: copy, the whole paragraph, look up.
    QQC2.Menu {
        id: srcMenu
        property string pid: ""
        property string selected: ""
        function show(pid, selected) {
            srcMenu.pid = pid;
            srcMenu.selected = selected.replace(/[\u2028\u2029]/g, "\n").trim();
            popup();
        }
        readonly property string lookup: selected.length > 0 && selected.length < 60 && selected.split(/\s+/).length <= 5 ? selected : ""
        QQC2.MenuItem {
            text: "Copy"
            icon.name: "edit-copy"
            enabled: srcMenu.selected !== ""
            onTriggered: Backend.copyText(srcMenu.selected)
        }
        QQC2.MenuItem {
            text: "Copy Paragraph Text"
            icon.name: "edit-copy"
            onTriggered: {
                const r = view.all[view.at[srcMenu.pid]];
                if (r)
                    Backend.copyText(view.textOf(r.src));
            }
        }
        QQC2.MenuItem {
            text: srcMenu.lookup ? "Look Up “" + srcMenu.lookup + "” in Merriam-Webster" : "Look Up in Merriam-Webster"
            icon.name: "edit-find"
            enabled: srcMenu.lookup !== ""
            onTriggered: Qt.openUrlExternally("https://www.merriam-webster.com/dictionary/" + encodeURIComponent(srcMenu.lookup))
        }
    }
}
