import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import se.cptjanst.babel

// Both documents as Babel's own layout engine lays them out, side by side and
// scrolling together. The translation is edited here: click, drag or
// double-click to place the caret or select (within a paragraph), type (input
// methods included), Backspace/Delete, arrows with Shift to select,
// Shift+Enter for a line break, Ctrl+Z/Y, Ctrl+B/I/U, Ctrl+A, Ctrl+C/X/V.
// Spelling and grammar are checked as you go. Saved a moment after typing stops.
Kirigami.Page {
    id: page

    property string projectId: ""
    property string projectName: ""
    property var source: []
    property var caret: null
    property var selection: []
    property var styles: ({ b: false, i: false, u: false })
    property bool canUndo: false
    property bool canRedo: false
    property var underlines: []
    property string proofStatus: ""
    property int issueCount: 0
    property bool proofing: false
    property real zoom: 1.0
    property bool busy: false
    property bool editing: false
    property bool unsaved: false
    property bool syncing: false

    readonly property real pxPerPt: zoom * 96 / 72

    title: projectName ? projectName + " — native layout" : "Native layout"
    padding: 0

    function load(id, name) {
        if (editing)
            Backend.nativeClose();
        projectId = id;
        projectName = name;
        source = [];
        target.clear();
        caret = null;
        selection = [];
        underlines = [];
        busy = true;
        Backend.renderPages(id);
        const r = JSON.parse(Backend.nativeOpen(id));
        editing = !!r;
        if (r) {
            setPages(r.pages);
            proofing = r.proofing;
        }
        ime.forceActiveFocus();
    }

    function close() {
        if (editing)
            Backend.nativeClose();
        editing = false;
        unsaved = false;
        issuePopup.close();
    }

    function setPages(pages) {
        target.clear();
        for (const p of pages)
            target.append(p);
    }

    function follow(from, to) {
        if (syncing)
            return;
        syncing = true;
        to.contentY = from.contentY;
        syncing = false;
    }

    // Takes an answer from the backend: caret, selection, pages drawn again.
    function answer(json, fromClick) {
        const r = JSON.parse(json);
        if (!r)
            return;
        if (r.pages) {
            if (r.all)
                setPages(r.pages);
            else
                for (const p of r.pages)
                    target.setProperty(p.index, "url", p.url);
            if (r.pages.length) {
                unsaved = true;
                refreshIssues();
            }
        }
        if (r.error)
            applicationWindow().showPassiveNotification(r.error, "short");
        caret = r.caret;
        selection = r.selection || [];
        if (r.styles)
            styles = r.styles;
        canUndo = !!r.canUndo;
        canRedo = !!r.canRedo;
        blink.restart();
        caretBox.visible = true;
        showCaret();
        if (fromClick && r.issue)
            issuePopup.show(r.issue);
        else
            issuePopup.close();
    }

    function refreshIssues() {
        const r = JSON.parse(Backend.nativeIssues());
        underlines = r.underlines;
        issueCount = r.count || 0;
        proofStatus = r.status || "";
    }

    function key(k, text, modifiers) {
        if (!editing || !caret)
            return false;
        const r = Backend.nativeKey(k, text, modifiers);
        if (r === "null")
            return false;
        answer(r, false);
        return true;
    }

    function copy(cut) {
        const r = Backend.nativeCopy(cut);
        if (r !== "null")
            answer(r, false);
    }

    function paste() {
        const r = Backend.nativePaste();
        if (r !== "null")
            answer(r, false);
    }

    // The caret's place in the page's coordinates (for the input method).
    function caretPoint() {
        if (!caret)
            return Qt.point(0, 0);
        const item = right.itemAtIndex(caret.page);
        if (!item)
            return Qt.point(0, 0);
        return item.mapToItem(page.contentItem, item.pageX + caret.x * pxPerPt, caret.top * pxPerPt);
    }

    // Scrolls the translation so the caret is in view.
    function showCaret() {
        if (!caret)
            return;
        const item = right.itemAtIndex(caret.page);
        if (!item) {
            right.positionViewAtIndex(caret.page, ListView.Contain);
            return;
        }
        const top = item.y + caret.top * pxPerPt;
        const bottom = top + caret.height * pxPerPt;
        if (top < right.contentY + 20)
            right.contentY = top - 40;
        else if (bottom > right.contentY + right.height - 20)
            right.contentY = bottom - right.height + 40;
    }

    Component.onDestruction: close()

    // Test hook (babel-desktop --project=ID --native --native-type=TEXT): puts
    // the caret near the top of page 1's text and types TEXT through the same
    // path as committed keyboard text.
    function typeTest(text) {
        page.answer(Backend.nativeClick(0, 100, 150, false), false);
        for (const ch of text)
            page.answer(Backend.nativeText(ch), false);
        // --native-type=…+ also selects the word before the caret and makes it bold.
        if (text.endsWith("+")) {
            page.key(Qt.Key_Backspace, "", 0);
            for (let i = 0; i < 6; i++)
                page.key(Qt.Key_Left, "", Qt.ShiftModifier);
            page.key(0x42, "", Qt.ControlModifier);
        }
    }

    actions: [
        Kirigami.Action {
            icon.name: "edit-undo"
            text: "Undo"
            displayHint: Kirigami.DisplayHint.IconOnly
            enabled: page.canUndo
            onTriggered: page.key(0x5A, "", 0x04000000)
        },
        Kirigami.Action {
            icon.name: "edit-redo"
            text: "Redo"
            displayHint: Kirigami.DisplayHint.IconOnly
            enabled: page.canRedo
            onTriggered: page.key(0x59, "", 0x04000000)
        },
        Kirigami.Action {
            icon.name: "format-text-bold"
            text: "Bold"
            displayHint: Kirigami.DisplayHint.IconOnly
            checkable: true
            checked: page.styles.b
            enabled: page.selection.length > 0
            onTriggered: page.key(0x42, "", 0x04000000)
        },
        Kirigami.Action {
            icon.name: "format-text-italic"
            text: "Italic"
            displayHint: Kirigami.DisplayHint.IconOnly
            checkable: true
            checked: page.styles.i
            enabled: page.selection.length > 0
            onTriggered: page.key(0x49, "", 0x04000000)
        },
        Kirigami.Action {
            icon.name: "format-text-underline"
            text: "Underline"
            displayHint: Kirigami.DisplayHint.IconOnly
            checkable: true
            checked: page.styles.u
            enabled: page.selection.length > 0
            onTriggered: page.key(0x55, "", 0x04000000)
        },
        Kirigami.Action {
            icon.name: "zoom-out"
            text: "Zoom Out"
            displayHint: Kirigami.DisplayHint.IconOnly
            onTriggered: page.zoom = Math.max(0.4, page.zoom / 1.2)
        },
        Kirigami.Action {
            icon.name: "zoom-in"
            text: "Zoom In"
            displayHint: Kirigami.DisplayHint.IconOnly
            onTriggered: page.zoom = Math.min(3, page.zoom * 1.2)
        }
    ]

    ListModel {
        id: target
    }

    Connections {
        target: Backend
        function onPagesReady(id, json) {
            if (id !== page.projectId)
                return;
            page.source = JSON.parse(json).source;
            page.busy = false;
        }
        function onMessage(text, error) {
            if (error)
                page.busy = false;
        }
        function onNativeIssuesChanged() {
            page.refreshIssues();
        }
        // Saved elsewhere (sync, the web editor): show it, unless something here is unsaved
        // (then the save merges, and the merged state comes back with it).
        function onNativeExternalChange(id) {
            if (id !== page.projectId || page.unsaved)
                return;
            const r = Backend.nativeReload(id);
            if (r !== "null") {
                page.answer(r, false);
                page.unsaved = false;
            }
        }
    }

    // Saves a second after the last change, and when leaving.
    Timer {
        id: saveTimer
        interval: 1000
        running: page.unsaved
        onTriggered: {
            Backend.nativeSave();
            page.unsaved = false;
        }
    }

    header: QQC2.Control {
        padding: Kirigami.Units.smallSpacing
        leftPadding: Kirigami.Units.largeSpacing
        background: Rectangle { color: Kirigami.Theme.alternateBackgroundColor }
        contentItem: RowLayout {
            QQC2.Label {
                Layout.fillWidth: true
                text: "Original"
                color: Kirigami.Theme.disabledTextColor
            }
            QQC2.Label {
                Layout.fillWidth: true
                elide: Text.ElideRight
                color: Kirigami.Theme.disabledTextColor
                text: {
                    if (!page.editing)
                        return "Translation";
                    let t = "Translation" + (page.unsaved ? " · saving…" : " · saved");
                    if (page.proofStatus)
                        t += " · " + page.proofStatus;
                    else if (page.proofing)
                        t += " · " + (page.issueCount ? page.issueCount + (page.issueCount === 1 ? " issue" : " issues") : "no issues");
                    return t;
                }
            }
        }
    }

    Rectangle {
        anchors.fill: parent
        color: Qt.darker(Kirigami.Theme.backgroundColor, 1.08)
    }

    component PageFrame: Rectangle {
        color: "transparent"
        border.color: Kirigami.Theme.disabledTextColor
        opacity: 0.4
    }

    RowLayout {
        anchors.fill: parent
        spacing: 0

        ListView {
            id: left
            Layout.fillWidth: true
            Layout.fillHeight: true
            clip: true
            spacing: Kirigami.Units.largeSpacing
            topMargin: Kirigami.Units.largeSpacing
            bottomMargin: Kirigami.Units.largeSpacing
            model: page.source
            QQC2.ScrollBar.vertical: QQC2.ScrollBar {}
            onContentYChanged: page.follow(left, right)
            delegate: Item {
                required property var modelData
                width: ListView.view.width
                height: modelData.h * page.pxPerPt
                PageFrame {
                    anchors.centerIn: simg
                    width: simg.width + 2
                    height: simg.height + 2
                }
                Image {
                    id: simg
                    anchors.horizontalCenter: parent.horizontalCenter
                    width: modelData.w * page.pxPerPt
                    height: modelData.h * page.pxPerPt
                    source: modelData.url
                    asynchronous: true
                    mipmap: true
                    cache: false
                }
            }
        }

        Kirigami.Separator { Layout.fillHeight: true }

        ListView {
            id: right
            Layout.fillWidth: true
            Layout.fillHeight: true
            clip: true
            spacing: Kirigami.Units.largeSpacing
            topMargin: Kirigami.Units.largeSpacing
            bottomMargin: Kirigami.Units.largeSpacing
            model: target
            cacheBuffer: 4000
            QQC2.ScrollBar.vertical: QQC2.ScrollBar {}
            onContentYChanged: page.follow(right, left)
            delegate: Item {
                id: tpage
                required property int index
                required property string url
                required property real w
                required property real h
                readonly property real pageX: timg.x
                width: ListView.view.width
                height: h * page.pxPerPt
                PageFrame {
                    anchors.centerIn: timg
                    width: timg.width + 2
                    height: timg.height + 2
                }
                Image {
                    id: timg
                    anchors.horizontalCenter: parent.horizontalCenter
                    width: tpage.w * page.pxPerPt
                    height: tpage.h * page.pxPerPt
                    source: tpage.url
                    // The page drawn before stays up until the new one is ready: no flicker.
                    asynchronous: false
                    mipmap: true
                    cache: false

                    // Selection.
                    Repeater {
                        model: page.selection.filter(r => r.page === tpage.index)
                        Rectangle {
                            required property var modelData
                            x: modelData.x * page.pxPerPt
                            y: modelData.y * page.pxPerPt
                            width: Math.max(2, modelData.w * page.pxPerPt)
                            height: modelData.h * page.pxPerPt
                            color: Kirigami.Theme.highlightColor
                            opacity: 0.35
                        }
                    }
                    // Proofing underlines.
                    Repeater {
                        model: page.underlines.filter(u => u.page === tpage.index)
                        Rectangle {
                            required property var modelData
                            x: modelData.x * page.pxPerPt
                            y: modelData.y * page.pxPerPt
                            width: modelData.w * page.pxPerPt
                            height: Math.max(1.5, 1.5 * page.pxPerPt)
                            color: modelData.kind === "spelling" ? "#d93025" : modelData.kind === "style" ? "#188038" : "#1a73e8"
                            opacity: 0.85
                        }
                    }
                    MouseArea {
                        anchors.fill: parent
                        cursorShape: Qt.IBeamCursor
                        acceptedButtons: Qt.LeftButton | Qt.RightButton
                        property bool dragging: false
                        onPressed: mouse => {
                            ime.forceActiveFocus();
                            if (mouse.button === Qt.RightButton) {
                                contextMenu.popup();
                                return;
                            }
                            dragging = true;
                            const extend = (mouse.modifiers & Qt.ShiftModifier) !== 0;
                            page.answer(Backend.nativeClick(tpage.index, mouse.x / page.pxPerPt, mouse.y / page.pxPerPt, extend), !extend);
                        }
                        onPositionChanged: mouse => {
                            if (dragging && (mouse.buttons & Qt.LeftButton))
                                page.answer(Backend.nativeClick(tpage.index, mouse.x / page.pxPerPt, mouse.y / page.pxPerPt, true), false);
                        }
                        onReleased: dragging = false
                        onDoubleClicked: mouse => page.answer(Backend.nativeWord(tpage.index, mouse.x / page.pxPerPt, mouse.y / page.pxPerPt), false)
                    }
                    Rectangle {
                        visible: page.caret !== null && page.caret.page === tpage.index && caretBox.visible && page.selection.length === 0
                        x: page.caret ? page.caret.x * page.pxPerPt - 1 : 0
                        y: page.caret ? page.caret.top * page.pxPerPt : 0
                        width: 2
                        height: page.caret ? page.caret.height * page.pxPerPt : 0
                        color: page.caret && !page.caret.editable ? Kirigami.Theme.disabledTextColor : Kirigami.Theme.textColor
                    }
                }
            }
        }
    }

    // Blinks the caret (the state lives here; each page draws its own mark).
    Item {
        id: caretBox
        visible: true
        Timer {
            id: blink
            interval: 530
            repeat: true
            running: page.caret !== null && ime.activeFocus
            onTriggered: caretBox.visible = !caretBox.visible
        }
    }

    // Takes the keyboard, and input methods, for the translation. It sits at
    // the caret (so candidate windows open there) but draws nothing: text it
    // receives is passed on and cleared; text being composed shows below.
    TextInput {
        id: ime
        focus: true
        opacity: 0
        width: 2
        height: page.caret ? page.caret.height * page.pxPerPt : 16
        x: page.caretPoint().x
        y: page.caretPoint().y
        font.pixelSize: Math.max(8, height * 0.8)
        onTextChanged: {
            if (text === "")
                return;
            const t = text;
            text = "";
            if (page.editing && page.caret)
                page.answer(Backend.nativeText(t), false);
        }
        Keys.onPressed: event => {
            const ctrl = (event.modifiers & Qt.ControlModifier) !== 0;
            if (ctrl && event.key === Qt.Key_C) { page.copy(false); event.accepted = true; return; }
            if (ctrl && event.key === Qt.Key_X) { page.copy(true); event.accepted = true; return; }
            if (ctrl && event.key === Qt.Key_V) { page.paste(); event.accepted = true; return; }
            // Text goes the input method's way (onTextChanged); everything else to the editor.
            const special = ctrl || event.text === "" || event.key === Qt.Key_Backspace || event.key === Qt.Key_Delete
                || event.key === Qt.Key_Return || event.key === Qt.Key_Enter || event.key === Qt.Key_Tab;
            if (event.key === Qt.Key_Tab) {
                if (page.key(0, "\t", 0))
                    event.accepted = true;
                return;
            }
            if (special && page.key(event.key, event.text, event.modifiers))
                event.accepted = true;
        }
    }

    // What the input method is composing, at the caret.
    QQC2.Label {
        visible: ime.preeditText !== ""
        text: ime.preeditText
        x: page.caretPoint().x
        y: page.caretPoint().y
        font.pixelSize: ime.font.pixelSize
        font.underline: true
        padding: 1
        background: Rectangle { color: Kirigami.Theme.backgroundColor; opacity: 0.9 }
    }

    QQC2.Menu {
        id: contextMenu
        QQC2.MenuItem { text: "Cut"; icon.name: "edit-cut"; enabled: page.selection.length > 0; onTriggered: page.copy(true) }
        QQC2.MenuItem { text: "Copy"; icon.name: "edit-copy"; enabled: page.selection.length > 0; onTriggered: page.copy(false) }
        QQC2.MenuItem { text: "Paste"; icon.name: "edit-paste"; onTriggered: page.paste() }
        QQC2.MenuSeparator {}
        QQC2.MenuItem { text: "Undo"; icon.name: "edit-undo"; enabled: page.canUndo; onTriggered: page.key(0x5A, "", 0x04000000) }
        QQC2.MenuItem { text: "Redo"; icon.name: "edit-redo"; enabled: page.canRedo; onTriggered: page.key(0x59, "", 0x04000000) }
        QQC2.MenuItem { text: "Select Paragraph"; onTriggered: page.key(0x41, "", 0x04000000) }
    }

    // A proofing issue under the caret after a click: what it is, suggestions, Ignore.
    QQC2.Popup {
        id: issuePopup
        property var issue: null
        function show(i) {
            issue = i;
            const p = page.caretPoint();
            x = Math.min(page.width - width - 8, Math.max(8, p.x - 20));
            y = p.y + (page.caret ? page.caret.height * page.pxPerPt : 16) + 6;
            open();
        }
        padding: Kirigami.Units.largeSpacing
        width: Kirigami.Units.gridUnit * 18
        closePolicy: QQC2.Popup.CloseOnEscape | QQC2.Popup.CloseOnPressOutside
        contentItem: ColumnLayout {
            spacing: Kirigami.Units.smallSpacing
            QQC2.Label {
                Layout.fillWidth: true
                wrapMode: Text.Wrap
                font.weight: Font.DemiBold
                text: issuePopup.issue ? (issuePopup.issue.short || (issuePopup.issue.kind === "spelling" ? "Spelling" : "Grammar")) : ""
            }
            QQC2.Label {
                Layout.fillWidth: true
                wrapMode: Text.Wrap
                text: issuePopup.issue ? issuePopup.issue.message : ""
            }
            Flow {
                Layout.fillWidth: true
                spacing: Kirigami.Units.smallSpacing
                Repeater {
                    model: issuePopup.issue ? issuePopup.issue.replacements : []
                    QQC2.Button {
                        required property string modelData
                        text: modelData === "" ? "(remove)" : modelData
                        onClicked: {
                            const i = issuePopup.issue;
                            issuePopup.close();
                            page.answer(Backend.nativeApply(i.pid, i.n, modelData), false);
                            ime.forceActiveFocus();
                        }
                    }
                }
            }
            QQC2.Button {
                text: "Ignore"
                flat: true
                onClicked: {
                    const i = issuePopup.issue;
                    issuePopup.close();
                    Backend.nativeIgnore(i.pid, i.n);
                    page.refreshIssues();
                    ime.forceActiveFocus();
                }
            }
        }
    }

    QQC2.BusyIndicator {
        anchors.centerIn: parent
        running: page.busy
        visible: running
    }
}
