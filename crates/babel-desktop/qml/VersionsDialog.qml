import QtCore
import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Dialogs
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import se.cptjanst.babel

// Versions of a translation: made every few minutes of work, at the start of
// each session, on export and before a restore; or named by you.
Kirigami.Dialog {
    id: dialog

    property string projectId: ""
    signal restored()

    title: "Versions"
    preferredWidth: Kirigami.Units.gridUnit * 30
    preferredHeight: Kirigami.Units.gridUnit * 28
    padding: Kirigami.Units.largeSpacing
    standardButtons: Kirigami.Dialog.Close

    function openFor(id) {
        projectId = id;
        reload();
        open();
    }

    function reload() {
        model.clear();
        for (const v of JSON.parse(Backend.versions(projectId))) {
            const st = v.stats || {};
            model.append({
                sid: v.sid,
                time: v.time,
                when: new Date(v.time).toLocaleString(Qt.locale(), Locale.ShortFormat),
                reason: v.reason === "auto" ? "automatic" : v.reason,
                info: (st.total ? Math.floor(100 * st.done / st.total) : 0) + " % · " + Number(st.tgtChars || 0).toLocaleString(Qt.locale(), "f", 0) + " characters",
            });
        }
    }

    ListModel {
        id: model
    }

    FileDialog {
        id: saveAs
        property string sid
        title: "Export version"
        fileMode: FileDialog.SaveFile
        nameFilters: ["Word documents (*.docx)"]
        onAccepted: Backend.exportProject(dialog.projectId, sid, selectedFile)
    }

    Kirigami.PromptDialog {
        id: confirm
        property string sid
        property string when
        title: "Restore this version?"
        subtitle: "The translation goes back to how it was on " + when + ". How it is now is kept as a version first."
        standardButtons: Kirigami.Dialog.Ok | Kirigami.Dialog.Cancel
        onAccepted: {
            Backend.restoreVersion(dialog.projectId, sid);
            dialog.close();
            dialog.restored();
        }
    }

    ColumnLayout {
        spacing: Kirigami.Units.largeSpacing
        RowLayout {
            Layout.fillWidth: true
            QQC2.TextField {
                id: name
                Layout.fillWidth: true
                placeholderText: "Name for a version of how it is now"
                onAccepted: keep.clicked()
            }
            QQC2.Button {
                id: keep
                text: "Keep Version"
                icon.name: "document-save"
                onClicked: {
                    Backend.addVersion(name.text);
                    name.text = "";
                    dialog.reload();
                }
            }
        }
        QQC2.ScrollView {
            Layout.fillWidth: true
            Layout.fillHeight: true
            Layout.preferredHeight: Kirigami.Units.gridUnit * 20
            ListView {
                model: model
                clip: true
                Kirigami.PlaceholderMessage {
                    anchors.centerIn: parent
                    width: parent.width - Kirigami.Units.gridUnit * 2
                    visible: parent.count === 0
                    text: "No versions yet"
                    explanation: "The first one is made after a few minutes of work."
                }
                delegate: QQC2.ItemDelegate {
                    id: row
                    required property string sid
                    required property double time
                    required property string when
                    required property string reason
                    required property string info
                    width: ListView.view.width
                    contentItem: RowLayout {
                        ColumnLayout {
                            Layout.fillWidth: true
                            spacing: 0
                            QQC2.Label { text: row.when; font.weight: Font.DemiBold }
                            QQC2.Label { text: row.reason + " · " + row.info; color: Kirigami.Theme.disabledTextColor; font: Kirigami.Theme.smallFont }
                        }
                        QQC2.ToolButton {
                            icon.name: "document-export"
                            text: "Export"
                            display: QQC2.AbstractButton.IconOnly
                            QQC2.ToolTip.text: "Export this version as .docx"
                            QQC2.ToolTip.visible: hovered
                            onClicked: {
                                saveAs.sid = row.sid;
                                saveAs.selectedFile = "file://" + StandardPaths.writableLocation(StandardPaths.DocumentsLocation).toString().replace(/^file:\/\//, "") + "/" + Backend.exportName(dialog.projectId, row.time);
                                saveAs.open();
                            }
                        }
                        QQC2.ToolButton {
                            icon.name: "edit-undo"
                            text: "Restore"
                            onClicked: {
                                confirm.sid = row.sid;
                                confirm.when = row.when;
                                confirm.open();
                            }
                        }
                    }
                }
            }
        }
    }
}
