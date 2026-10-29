import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Dialogs
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

// Continue from a translated file: the original and a translation of it
// (downloaded from Babel earlier, or with the same paragraph structure).
Kirigami.Dialog {
    id: dialog

    property url original
    property url translated
    signal pair(url original, url translated)

    title: "Continue from a Translated File"
    preferredWidth: Kirigami.Units.gridUnit * 26
    padding: Kirigami.Units.largeSpacing
    standardButtons: Kirigami.Dialog.NoButton
    customFooterActions: [
        Kirigami.Action {
            text: "Pair and Open"
            icon.name: "go-next"
            enabled: dialog.original.toString() !== "" && dialog.translated.toString() !== ""
            onTriggered: {
                dialog.close();
                dialog.pair(dialog.original, dialog.translated);
            }
        },
        Kirigami.Action {
            text: "Cancel"
            icon.name: "dialog-cancel"
            onTriggered: dialog.close()
        }
    ]

    function name(u) {
        return u.toString() ? decodeURIComponent(u.toString().split("/").pop()) : "";
    }

    FileDialog {
        id: pick
        property bool forOriginal: true
        nameFilters: ["Word documents (*.docx)"]
        onAccepted: if (forOriginal) dialog.original = selectedFile; else dialog.translated = selectedFile
    }

    ColumnLayout {
        spacing: Kirigami.Units.largeSpacing
        QQC2.Label {
            Layout.fillWidth: true
            wrapMode: Text.Wrap
            text: "If you have the original and a translation previously exported from Babel (or with the same paragraph structure), pair them to pick up where you left off."
        }
        Kirigami.FormLayout {
            Layout.fillWidth: true
            RowLayout {
                Kirigami.FormData.label: "Original:"
                QQC2.Label { text: dialog.name(dialog.original) || "—"; elide: Text.ElideMiddle; Layout.maximumWidth: Kirigami.Units.gridUnit * 12 }
                QQC2.Button { text: "Choose…"; onClicked: { pick.forOriginal = true; pick.title = "The original"; pick.open(); } }
            }
            RowLayout {
                Kirigami.FormData.label: "Translation:"
                QQC2.Label { text: dialog.name(dialog.translated) || "—"; elide: Text.ElideMiddle; Layout.maximumWidth: Kirigami.Units.gridUnit * 12 }
                QQC2.Button { text: "Choose…"; onClicked: { pick.forOriginal = false; pick.title = "The translation"; pick.open(); } }
            }
        }
    }
}
