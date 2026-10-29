import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

// Keys and how Babel works (as the web editor's help).
Kirigami.Dialog {
    id: dialog

    title: "Babel"
    preferredWidth: Kirigami.Units.gridUnit * 30
    padding: Kirigami.Units.largeSpacing
    standardButtons: Kirigami.Dialog.Close

    readonly property var keys: [
        ["Click, drag", "Place the caret, select (within a paragraph)"],
        ["Double-click", "Select a word"],
        ["Shift + arrows, Home, End", "Select"],
        ["Enter", "Next paragraph"],
        ["Shift + Enter", "Line break"],
        ["Ctrl + Enter", "Mark the paragraph as done (or not) and go to the next"],
        ["Ctrl + B, I, U", "Bold, italic, underline the selection"],
        ["Ctrl + Z, Ctrl + Y", "Undo, redo"],
        ["Ctrl + C, X, V", "Copy, cut, paste as plain text"],
        ["Ctrl + K", "Make the selection a link (to the link you copied in the original, or an address you copied)"],
        ["Ctrl + A", "Select the paragraph"],
        ["Right-click", "In the translation: revert, copy the original. In the original: copy, look up in Merriam-Webster, copy a link"],
        ["Click in the original", "Go to the paragraph's translation"],
    ]

    ColumnLayout {
        spacing: Kirigami.Units.largeSpacing
        QQC2.Label {
            Layout.fillWidth: true
            wrapMode: Text.Wrap
            text: "Type over the translation paragraph by paragraph. The exported .docx is the original file with the words changed: same styles, numbering, tables, pictures, headers, footers, footnotes and fields. Paragraphs are never split or merged."
        }
        GridLayout {
            columns: 2
            columnSpacing: Kirigami.Units.largeSpacing
            rowSpacing: Kirigami.Units.smallSpacing
            Repeater {
                model: dialog.keys.length * 2
                QQC2.Label {
                    required property int index
                    readonly property var row: dialog.keys[Math.floor(index / 2)]
                    Layout.fillWidth: index % 2 === 1
                    Layout.alignment: Qt.AlignTop
                    wrapMode: index % 2 ? Text.Wrap : Text.NoWrap
                    font.weight: index % 2 ? Font.Normal : Font.DemiBold
                    text: row[index % 2]
                }
            }
        }
        QQC2.Label {
            Layout.fillWidth: true
            wrapMode: Text.Wrap
            color: Kirigami.Theme.disabledTextColor
            text: "Your translations are kept on this computer and, when you are signed in, synced with your Babel account. Pages are laid out by Babel's own engine, built on WordCraft (MIT or Apache-2.0)."
        }
    }
}
