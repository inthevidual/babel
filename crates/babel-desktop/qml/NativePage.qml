import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import se.cptjanst.babel

// Both documents as the native layout engine lays them out (read-only, as
// last saved), side by side, scrolling together.
Kirigami.Page {
    id: page

    property string projectId: ""
    property string projectName: ""
    property var source: []
    property var target: []
    property real zoom: 1.0
    property bool busy: false
    property string info: ""
    property bool syncing: false

    function follow(from, to) {
        if (syncing)
            return;
        syncing = true;
        to.contentY = from.contentY;
        syncing = false;
    }

    title: projectName ? projectName + " — native layout" : "Native layout"
    padding: 0

    function load(id, name) {
        projectId = id;
        projectName = name;
        source = [];
        target = [];
        busy = true;
        Backend.renderPages(id);
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
        },
        Kirigami.Action {
            icon.name: "view-refresh"
            text: "Lay Out Again"
            enabled: !page.busy
            onTriggered: page.load(page.projectId, page.projectName)
        }
    ]

    Connections {
        target: Backend
        function onPagesReady(id, json) {
            if (id !== page.projectId)
                return;
            const r = JSON.parse(json);
            page.source = r.source;
            page.target = r.target;
            page.busy = false;
            page.info = r.source.length + " / " + r.target.length + " pages · laid out in " + (r.ms / 1000).toFixed(1) + " s";
        }
        function onMessage(text, error) {
            if (error)
                page.busy = false;
        }
    }

    header: QQC2.Control {
        padding: Kirigami.Units.smallSpacing
        leftPadding: Kirigami.Units.largeSpacing
        background: Rectangle { color: Kirigami.Theme.alternateBackgroundColor }
        contentItem: QQC2.Label {
            text: page.busy ? "Laying out…" : "Original · Translation as last saved · " + page.info
            color: Kirigami.Theme.disabledTextColor
        }
    }

    component Pages: ListView {
        id: lv
        required property var pages
        clip: true
        spacing: Kirigami.Units.largeSpacing
        topMargin: Kirigami.Units.largeSpacing
        bottomMargin: Kirigami.Units.largeSpacing
        model: pages
        QQC2.ScrollBar.vertical: QQC2.ScrollBar {}
        delegate: Item {
            required property var modelData
            width: ListView.view.width
            height: modelData.h * page.zoom * 96 / 72
            Rectangle {
                anchors.centerIn: img
                width: img.width + 2
                height: img.height + 2
                color: "transparent"
                border.color: Kirigami.Theme.disabledTextColor
                opacity: 0.4
            }
            Image {
                id: img
                anchors.horizontalCenter: parent.horizontalCenter
                width: modelData.w * page.zoom * 96 / 72
                height: modelData.h * page.zoom * 96 / 72
                source: modelData.url
                asynchronous: true
                smooth: true
                mipmap: true
                cache: false
            }
        }
    }

    Rectangle {
        anchors.fill: parent
        color: Qt.darker(Kirigami.Theme.backgroundColor, 1.08)
    }

    RowLayout {
        anchors.fill: parent
        spacing: 0
        Pages {
            id: left
            pages: page.source
            Layout.fillWidth: true
            Layout.fillHeight: true
            onContentYChanged: page.follow(left, right)
        }
        Kirigami.Separator { Layout.fillHeight: true }
        Pages {
            id: right
            pages: page.target
            Layout.fillWidth: true
            Layout.fillHeight: true
            onContentYChanged: page.follow(right, left)
        }
    }

    QQC2.BusyIndicator {
        anchors.centerIn: parent
        running: page.busy
        visible: running
    }
}
