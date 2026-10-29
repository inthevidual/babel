import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import se.cptjanst.babel

Kirigami.ScrollablePage {
    id: page

    signal openRequested(string id, string name)
    signal pairRequested()
    signal importRequested()
    signal fileDropped(url url)
    signal exportRequested(string id)
    signal accountRequested()

    title: "Translations"

    actions: [
        Kirigami.Action {
            text: "Open Document…"
            icon.name: "document-open"
            onTriggered: page.importRequested()
        },
        Kirigami.Action {
            text: Backend.signedIn ? "Sync Now" : "Sign In to Sync…"
            icon.name: Backend.signedIn ? "view-refresh" : "folder-cloud"
            enabled: !Backend.syncing
            onTriggered: Backend.signedIn ? Backend.syncNow() : page.accountRequested()
        },
        Kirigami.Action {
            text: "Continue from a Translated File…"
            icon.name: "document-import"
            displayHint: Kirigami.DisplayHint.AlwaysHide
            onTriggered: page.pairRequested()
        },
        Kirigami.Action {
            text: "Account…"
            icon.name: "user-identity"
            displayHint: Kirigami.DisplayHint.AlwaysHide
            onTriggered: page.accountRequested()
        }
    ]

    ListModel {
        id: model
    }

    function reload() {
        const list = JSON.parse(Backend.projects());
        model.clear();
        for (const p of list) {
            const st = p.stats || {};
            model.append({
                pid: p.id,
                name: p.name || p.fileName || "Untitled",
                lang: p.langName || p.targetLang || "",
                done: st.total ? Math.round(100 * (st.done || 0) / st.total) : 0,
                updated: p.updatedAt || 0,
            });
        }
    }

    function ago(t) {
        if (!t)
            return "";
        const s = (Date.now() - t) / 1000;
        if (s < 60)
            return "just now";
        if (s < 3600)
            return Math.floor(s / 60) + " min ago";
        if (s < 86400)
            return Math.floor(s / 3600) + " h ago";
        return new Date(t).toLocaleDateString(Qt.locale(), Locale.ShortFormat);
    }

    Component.onCompleted: reload()

    Connections {
        target: Backend
        function onProjectsChanged() {
            page.reload();
        }
    }

    header: QQC2.Control {
        visible: Backend.signedIn || Backend.syncStatus !== ""
        padding: Kirigami.Units.smallSpacing
        leftPadding: Kirigami.Units.largeSpacing
        background: Rectangle {
            color: Kirigami.Theme.alternateBackgroundColor
        }
        contentItem: RowLayout {
            Kirigami.Icon {
                source: Backend.syncing ? "view-refresh" : "folder-cloud"
                implicitWidth: Kirigami.Units.iconSizes.small
                implicitHeight: Kirigami.Units.iconSizes.small
            }
            QQC2.Label {
                Layout.fillWidth: true
                elide: Text.ElideRight
                text: (Backend.signedIn ? Backend.syncUser + " on " + Backend.syncServer : "") + (Backend.syncStatus ? " · " + Backend.syncStatus : "")
            }
        }
    }

    ListView {
        id: list
        model: model
        reuseItems: true
        currentIndex: -1

        delegate: QQC2.ItemDelegate {
            id: row
            required property string pid
            required property string name
            required property string lang
            required property int done
            required property double updated

            width: ListView.view.width
            onClicked: page.openRequested(pid, name)

            contentItem: RowLayout {
                spacing: Kirigami.Units.largeSpacing
                Kirigami.Icon {
                    source: "application-vnd.openxmlformats-officedocument.wordprocessingml.document"
                    fallback: "x-office-document"
                    implicitWidth: Kirigami.Units.iconSizes.medium
                    implicitHeight: Kirigami.Units.iconSizes.medium
                }
                ColumnLayout {
                    Layout.fillWidth: true
                    spacing: 2
                    QQC2.Label {
                        Layout.fillWidth: true
                        text: row.name
                        elide: Text.ElideRight
                        font.weight: Font.DemiBold
                    }
                    QQC2.Label {
                        Layout.fillWidth: true
                        text: "→ " + row.lang + " · " + row.done + " % translated · " + page.ago(row.updated)
                        elide: Text.ElideRight
                        color: Kirigami.Theme.disabledTextColor
                        font: Kirigami.Theme.smallFont
                    }
                    QQC2.ProgressBar {
                        Layout.fillWidth: true
                        Layout.maximumWidth: Kirigami.Units.gridUnit * 16
                        from: 0
                        to: 100
                        value: row.done
                    }
                }
                QQC2.ToolButton {
                    icon.name: "document-export"
                    text: "Export .docx"
                    display: QQC2.AbstractButton.IconOnly
                    QQC2.ToolTip.text: text
                    QQC2.ToolTip.visible: hovered
                    onClicked: page.exportRequested(row.pid)
                }
                QQC2.ToolButton {
                    icon.name: "edit-delete"
                    text: "Delete"
                    display: QQC2.AbstractButton.IconOnly
                    QQC2.ToolTip.text: text
                    QQC2.ToolTip.visible: hovered
                    onClicked: {
                        confirmDelete.pid = row.pid;
                        confirmDelete.name = row.name;
                        confirmDelete.open();
                    }
                }
            }
        }

        Kirigami.PlaceholderMessage {
            anchors.centerIn: parent
            width: parent.width - Kirigami.Units.gridUnit * 4
            visible: list.count === 0
            icon.name: "document-open"
            text: "No translations yet"
            explanation: "Open a Word document to start translating it side by side. The layout stays put; only the words change." + (Backend.signedIn ? "" : "\nSign in to a Babel server to sync your translations between computers and the web.")
            helpfulAction: Kirigami.Action {
                text: "Open Document…"
                icon.name: "document-open"
                onTriggered: page.importRequested()
            }
        }
    }

    DropArea {
        anchors.fill: parent
        keys: ["text/uri-list"]
        onDropped: drop => {
            for (const u of drop.urls) {
                if (u.toString().toLowerCase().endsWith(".docx")) {
                    page.fileDropped(u);
                    return;
                }
            }
        }
    }

    Kirigami.PromptDialog {
        id: confirmDelete
        property string pid
        property string name
        title: "Delete “" + name + "”?"
        subtitle: "The translation and all its versions are deleted" + (Backend.signedIn ? ", here and on the server." : ". Export it first if you want to keep it.")
        standardButtons: Kirigami.Dialog.NoButton
        customFooterActions: [
            Kirigami.Action {
                text: "Delete"
                icon.name: "edit-delete"
                onTriggered: {
                    Backend.deleteProject(confirmDelete.pid);
                    confirmDelete.close();
                }
            },
            Kirigami.Action {
                text: "Cancel"
                icon.name: "dialog-cancel"
                onTriggered: confirmDelete.close()
            }
        ]
    }
}
