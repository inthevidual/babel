import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import se.cptjanst.babel

// Both documents as Babel's own layout engine lays them out, side by side and
// scrolling together; the editor is babel-editor (shared with the browser).
//
// The translation: click, drag or double-click to place the caret or select
// (within a paragraph), type (input methods included), Backspace/Delete,
// arrows with Shift to select, Shift+Enter for a line break, Enter for the
// next paragraph, Ctrl+Enter to mark it done, Ctrl+Z/Y, Ctrl+B/I/U, Ctrl+A,
// Ctrl+C/X/V, Ctrl+K to paste a link; right-click for the rest. Spelling and
// grammar are checked as you go. Saved a moment after typing stops.
//
// The original: a click goes to the translation's paragraph; select to copy
// or look up; right-click a link to copy it.
Kirigami.Page {
    id: page

    property string projectId: ""
    property string projectName: ""
    property var caret: null
    property var selection: []
    property var sourceSelection: []
    property var styles: ({ b: false, i: false, u: false })
    property var paragraph: null
    property var stats: null
    property bool canUndo: false
    property bool canRedo: false
    property var underlines: []
    property var marks: []
    property string proofStatus: ""
    property int issueCount: 0
    property bool proofing: false
    property real zoom: 1.0
    property bool editing: false
    property bool unsaved: false
    property bool syncing: false

    readonly property real pxPerPt: zoom * 96 / 72
    property bool showMarks: true
    property string targetLang: ""

    signal exportRequested(string id)
    signal versionsRequested(string id)
    signal languageRequested(string current)
    signal settingsRequested()
    signal helpRequested()

    function languageChanged(code) {
        targetLang = code;
        refreshIssues();
    }

    function settingsChanged(s) {
        showMarks = s.marks;
        proofing = s.proof;
        refreshIssues();
    }

    // After a version was restored: carry on from it.
    function reloadSoon() {
        reloadTimer.start();
    }

    Timer {
        id: reloadTimer
        interval: 300
        onTriggered: {
            const r = Backend.nativeReload(page.projectId);
            if (r !== "null")
                page.answer(r, false);
        }
    }

    title: projectName || "Translation"
    padding: 0

    function load(id, name) {
        if (editing)
            Backend.nativeClose();
        projectId = id;
        projectName = name;
        source.clear();
        target.clear();
        const r = JSON.parse(Backend.nativeOpen(id));
        editing = !!r;
        const st = JSON.parse(Backend.settings());
        showMarks = st.marks;
        if (r) {
            if (!projectName && r.name)
                projectName = r.name;
            targetLang = r.targetLang || "";
            for (const p of r.source)
                source.append(p);
            proofing = r.proofing && st.proof;
            r.all = true;
            answer(JSON.stringify(r), false);
            unsaved = false;
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

    function n(x) {
        return Number(x || 0).toLocaleString(Qt.locale(), "f", 0);
    }

    // Takes an answer from the backend: caret, selections, pages drawn again, marks.
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
        if (r.marks)
            marks = r.marks;
        if (r.error)
            applicationWindow().showPassiveNotification(r.error, "short");
        if (r.message)
            applicationWindow().showPassiveNotification(r.message, "short");
        caret = r.caret;
        selection = r.selection || [];
        sourceSelection = r.sourceSelection || [];
        if (r.styles)
            styles = r.styles;
        paragraph = r.paragraph || null;
        if (r.stats)
            stats = r.stats;
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

    function pasteLink() {
        answer(Backend.nativePasteLink(), false);
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
    // path as committed keyboard text. A trailing "+" is deleted again, the
    // six characters before the caret are selected and made bold.
    function typeTest(text) {
        page.answer(Backend.nativeClick(0, 100, 150, false), false);
        for (const ch of text)
            page.answer(Backend.nativeText(ch), false);
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
            icon.name: "insert-link"
            text: "Paste Link"
            displayHint: Kirigami.DisplayHint.IconOnly
            enabled: page.selection.length > 0
            onTriggered: page.pasteLink()
        },
        Kirigami.Action {
            icon.name: "checkmark"
            text: page.paragraph && page.paragraph.done ? "Unmark as Done" : "Mark as Done"
            displayHint: Kirigami.DisplayHint.IconOnly
            checkable: true
            checked: !!(page.paragraph && page.paragraph.done)
            enabled: page.paragraph !== null
            onTriggered: page.key(0x01000004, "", 0x04000000)
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
        },
        Kirigami.Action {
            icon.name: "document-export"
            text: "Export .docx…"
            displayHint: Kirigami.DisplayHint.KeepVisible
            onTriggered: {
                Backend.nativeSave();
                page.unsaved = false;
                page.exportRequested(page.projectId);
            }
        },
        Kirigami.Action {
            icon.name: "view-history"
            text: "Versions…"
            displayHint: Kirigami.DisplayHint.AlwaysHide
            onTriggered: {
                Backend.nativeSave();
                page.unsaved = false;
                page.versionsRequested(page.projectId);
            }
        },
        Kirigami.Action {
            icon.name: "languages"
            text: "Target Language…"
            displayHint: Kirigami.DisplayHint.AlwaysHide
            onTriggered: page.languageRequested(page.targetLang)
        },
        Kirigami.Action {
            icon.name: "configure"
            text: "Settings…"
            displayHint: Kirigami.DisplayHint.AlwaysHide
            onTriggered: page.settingsRequested()
        },
        Kirigami.Action {
            icon.name: "help-contents"
            text: "Keys and Help"
            displayHint: Kirigami.DisplayHint.AlwaysHide
            onTriggered: page.helpRequested()
        }
    ]

    ListModel {
        id: source
    }

    ListModel {
        id: target
    }

    Connections {
        target: Backend
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
                Layout.preferredWidth: 1
                elide: Text.ElideRight
                color: Kirigami.Theme.disabledTextColor
                text: page.stats ? "Original · " + page.n(page.stats.srcChars) + " characters with spaces · " + page.stats.srcPages + " pages" : "Original"
            }
            QQC2.Label {
                Layout.fillWidth: true
                Layout.preferredWidth: 1
                elide: Text.ElideRight
                color: Kirigami.Theme.disabledTextColor
                text: {
                    if (!page.editing || !page.stats)
                        return "Translation";
                    const s = page.stats;
                    const d = s.srcChars ? (100 * (s.tgtChars - s.srcChars) / s.srcChars) : 0;
                    let t = "Translation · " + page.n(s.tgtChars) + " characters" + (d ? " " + (d > 0 ? "+" : "") + d.toFixed(1) + " %" : "")
                        + " · " + (s.total ? Math.floor(100 * s.done / s.total) : 0) + " % done (" + s.done + "/" + s.total + ")";
                    t += page.unsaved ? " · saving…" : " · saved";
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

    component Highlight: Rectangle {
        required property var modelData
        x: modelData.x * page.pxPerPt
        y: modelData.y * page.pxPerPt
        width: Math.max(2, modelData.w * page.pxPerPt)
        height: modelData.h * page.pxPerPt
        color: Kirigami.Theme.highlightColor
        opacity: 0.35
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
            model: source
            cacheBuffer: 4000
            QQC2.ScrollBar.vertical: QQC2.ScrollBar {}
            onContentYChanged: page.follow(left, right)
            delegate: Item {
                id: spage
                required property int index
                required property string url
                required property real w
                required property real h
                width: ListView.view.width
                height: h * page.pxPerPt
                PageFrame {
                    anchors.centerIn: simg
                    width: simg.width + 2
                    height: simg.height + 2
                }
                Image {
                    id: simg
                    anchors.horizontalCenter: parent.horizontalCenter
                    width: spage.w * page.pxPerPt
                    height: spage.h * page.pxPerPt
                    source: spage.url
                    asynchronous: true
                    mipmap: true
                    cache: false
                    Repeater {
                        model: page.sourceSelection.filter(r => r.page === spage.index)
                        Highlight {}
                    }
                    MouseArea {
                        anchors.fill: parent
                        cursorShape: Qt.IBeamCursor
                        acceptedButtons: Qt.LeftButton | Qt.RightButton
                        property bool dragging: false
                        onPressed: mouse => {
                            if (mouse.button === Qt.RightButton) {
                                sourceMenu.show(JSON.parse(Backend.nativeSourceMenu(spage.index, mouse.x / page.pxPerPt, mouse.y / page.pxPerPt)));
                                return;
                            }
                            ime.forceActiveFocus();
                            dragging = true;
                            page.answer(Backend.nativeSourceClick(spage.index, mouse.x / page.pxPerPt, mouse.y / page.pxPerPt, (mouse.modifiers & Qt.ShiftModifier) !== 0), false);
                        }
                        onPositionChanged: mouse => {
                            if (dragging && (mouse.buttons & Qt.LeftButton))
                                page.answer(Backend.nativeSourceClick(spage.index, mouse.x / page.pxPerPt, mouse.y / page.pxPerPt, true), false);
                        }
                        onReleased: dragging = false
                        onDoubleClicked: mouse => page.answer(Backend.nativeSourceWord(spage.index, mouse.x / page.pxPerPt, mouse.y / page.pxPerPt), false)
                    }
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

                    // Edited (blue) and done (green) paragraphs, beside them.
                    Repeater {
                        model: page.showMarks ? page.marks.filter(m => m.page === tpage.index) : []
                        Rectangle {
                            required property var modelData
                            x: modelData.x * page.pxPerPt - 9 * page.pxPerPt
                            y: modelData.y * page.pxPerPt
                            width: Math.max(2, 2.5 * page.pxPerPt)
                            height: modelData.h * page.pxPerPt
                            radius: width / 2
                            color: modelData.kind === "done" ? "#2f9a5d" : "#3c6fd6"
                            opacity: 0.8
                        }
                    }
                    Repeater {
                        model: page.selection.filter(r => r.page === tpage.index)
                        Highlight {}
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
                                if (page.selection.length === 0)
                                    page.answer(Backend.nativeClick(tpage.index, mouse.x / page.pxPerPt, mouse.y / page.pxPerPt, false), false);
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
            if (ctrl && event.key === Qt.Key_C) {
                if (page.selection.length)
                    page.copy(false);
                else
                    Backend.nativeCopySource();
                event.accepted = true;
                return;
            }
            if (ctrl && event.key === Qt.Key_X) { page.copy(true); event.accepted = true; return; }
            if (ctrl && event.key === Qt.Key_V) { page.paste(); event.accepted = true; return; }
            if (ctrl && event.key === Qt.Key_K) { page.pasteLink(); event.accepted = true; return; }
            // Text goes the input method's way (onTextChanged); everything else to the editor.
            if (event.key === Qt.Key_Tab) {
                if (page.key(0, "\t", 0))
                    event.accepted = true;
                return;
            }
            const special = ctrl || event.text === "" || event.key === Qt.Key_Backspace || event.key === Qt.Key_Delete
                || event.key === Qt.Key_Return || event.key === Qt.Key_Enter;
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
        QQC2.MenuItem { text: "Paste as Plain Text"; icon.name: "edit-paste"; onTriggered: page.paste() }
        QQC2.MenuItem { text: "Paste Link"; icon.name: "insert-link"; enabled: page.selection.length > 0; onTriggered: page.pasteLink() }
        QQC2.MenuSeparator {}
        QQC2.MenuItem {
            text: page.paragraph && page.paragraph.done ? "Unmark as Done" : "Mark as Done"
            icon.name: "checkmark"
            enabled: page.paragraph !== null
            onTriggered: page.key(0x01000004, "", 0x04000000)
        }
        QQC2.MenuItem { text: "Revert Paragraph to the Original"; icon.name: "edit-reset"; enabled: page.paragraph !== null && page.paragraph.edited; onTriggered: page.answer(Backend.nativeRevert(), false) }
        QQC2.MenuItem { text: "Copy the Original Text"; icon.name: "edit-copy"; enabled: page.paragraph !== null; onTriggered: Backend.nativeCopyOriginal() }
        QQC2.MenuSeparator {}
        QQC2.MenuItem { text: "Undo"; icon.name: "edit-undo"; enabled: page.canUndo; onTriggered: page.key(0x5A, "", 0x04000000) }
        QQC2.MenuItem { text: "Redo"; icon.name: "edit-redo"; enabled: page.canRedo; onTriggered: page.key(0x59, "", 0x04000000) }
        QQC2.MenuItem { text: "Select Paragraph"; onTriggered: page.key(0x41, "", 0x04000000) }
    }

    // The original's menu: what the right-click found there.
    QQC2.Menu {
        id: sourceMenu
        property var found: ({})
        function show(f) {
            found = f || {};
            popup();
        }
        QQC2.MenuItem { text: "Copy"; icon.name: "edit-copy"; enabled: !!sourceMenu.found.text; onTriggered: Backend.nativeCopySource() }
        QQC2.MenuItem {
            text: sourceMenu.found.lookup ? "Look Up “" + sourceMenu.found.lookup + "” in Merriam-Webster" : "Look Up in Merriam-Webster"
            icon.name: "edit-find"
            enabled: !!sourceMenu.found.lookup
            onTriggered: Qt.openUrlExternally("https://www.merriam-webster.com/dictionary/" + encodeURIComponent(sourceMenu.found.lookup))
        }
        QQC2.MenuItem {
            text: sourceMenu.found.link ? "Copy Link  " + sourceMenu.found.link : "Copy Link"
            icon.name: "insert-link"
            enabled: !!sourceMenu.found.link
            onTriggered: Backend.nativeCopyLink()
        }
        QQC2.MenuItem { text: "Copy Paragraph Text"; icon.name: "edit-copy"; enabled: !!sourceMenu.found.paragraph; onTriggered: Backend.copyText(sourceMenu.found.paragraph) }
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
            RowLayout {
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
                QQC2.Button {
                    text: "Add to Dictionary"
                    flat: true
                    visible: issuePopup.issue !== null && issuePopup.issue.kind === "spelling"
                    onClicked: {
                        const i = issuePopup.issue;
                        issuePopup.close();
                        Backend.nativeAddWord(i.pid, i.n);
                        page.refreshIssues();
                        ime.forceActiveFocus();
                    }
                }
            }
        }
    }
}
