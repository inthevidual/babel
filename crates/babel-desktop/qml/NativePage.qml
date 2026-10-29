import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import se.cptjanst.babel

// Both documents as Babel's own layout engine lays them out, side by side and
// scrolling together. The translation can be edited here: click to place the
// caret, type, delete, move with the arrow keys. It is saved a moment after
// typing stops.
Kirigami.Page {
    id: page

    property string projectId: ""
    property string projectName: ""
    property var source: []
    property var caret: null
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
        busy = true;
        Backend.renderPages(id);
        const r = JSON.parse(Backend.nativeOpen(id));
        editing = !!r;
        if (r)
            setPages(r.pages);
        keys.forceActiveFocus();
    }

    function close() {
        if (editing)
            Backend.nativeClose();
        editing = false;
        unsaved = false;
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

    function answer(json) {
        const r = JSON.parse(json);
        if (!r)
            return;
        if (r.all)
            setPages(r.pages);
        else
            for (const p of r.pages || [])
                target.setProperty(p.index, "url", p.url);
        if ((r.pages || []).length)
            unsaved = true;
        if (r.error)
            applicationWindow().showPassiveNotification(r.error, "short");
        caret = r.caret;
        blink.restart();
        caretBox.visible = true;
        showCaret();
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
    // path as the keyboard.
    function typeTest(text) {
        page.answer(Backend.nativeClick(0, 100, 150));
        for (const ch of text)
            page.answer(Backend.nativeKey(0, ch));
    }

    actions: [
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
                text: page.editing ? "Translation — click to edit" + (page.unsaved ? " · saving…" : " · saved") : "Translation"
                color: Kirigami.Theme.disabledTextColor
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
                    MouseArea {
                        anchors.fill: parent
                        cursorShape: Qt.IBeamCursor
                        onPressed: mouse => {
                            keys.forceActiveFocus();
                            page.answer(Backend.nativeClick(tpage.index, mouse.x / page.pxPerPt, mouse.y / page.pxPerPt));
                        }
                    }
                    Rectangle {
                        id: caretMark
                        visible: page.caret !== null && page.caret.page === tpage.index && caretBox.visible
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
            running: page.caret !== null && keys.activeFocus
            onTriggered: caretBox.visible = !caretBox.visible
        }
    }

    // Takes the keyboard for the translation.
    Item {
        id: keys
        focus: true
        Keys.onPressed: event => {
            if (!page.editing || !page.caret)
                return;
            if (event.modifiers & (Qt.ControlModifier | Qt.AltModifier | Qt.MetaModifier))
                return;
            const r = Backend.nativeKey(event.key, event.text);
            if (r !== "null") {
                page.answer(r);
                event.accepted = true;
            }
        }
    }

    QQC2.BusyIndicator {
        anchors.centerIn: parent
        running: page.busy
        visible: running
    }
}
