import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import se.cptjanst.babel

// Picks a target language (the web editor's list, web/js/langs.js).
Kirigami.Dialog {
    id: dialog

    property string subtitle: ""
    property string current: ""
    property string okText: "Start Translating"
    property var all: []
    signal chosen(string code)

    preferredWidth: Kirigami.Units.gridUnit * 22
    preferredHeight: Kirigami.Units.gridUnit * 26
    padding: Kirigami.Units.largeSpacing
    standardButtons: Kirigami.Dialog.NoButton
    customFooterActions: [
        Kirigami.Action {
            text: dialog.okText
            icon.name: "dialog-ok"
            enabled: dialog.current !== ""
            onTriggered: dialog.accept()
        },
        Kirigami.Action {
            text: "Cancel"
            icon.name: "dialog-cancel"
            onTriggered: dialog.close()
        }
    ]

    function ask(title, subtitle, current, okText) {
        dialog.title = title;
        dialog.subtitle = subtitle || "";
        dialog.current = current || "";
        dialog.okText = okText || "Start Translating";
        if (!all.length)
            all = JSON.parse(Backend.languages());
        search.text = "";
        filter();
        open();
        search.forceActiveFocus();
    }

    function filter() {
        const q = search.text.trim().toLowerCase();
        model.clear();
        for (const l of all)
            if (!q || (l.name + " " + l.en + " " + l.code).toLowerCase().includes(q))
                model.append({ code: l.code, name: l.name, en: l.en, proofing: !!l.lt });
        for (let i = 0; i < model.count; i++)
            if (model.get(i).code === current)
                list.positionViewAtIndex(i, ListView.Center);
    }

    function accept() {
        if (!current)
            return;
        close();
        chosen(current);
    }

    ListModel {
        id: model
    }

    ColumnLayout {
        spacing: Kirigami.Units.smallSpacing
        QQC2.Label {
            Layout.fillWidth: true
            visible: dialog.subtitle !== ""
            text: dialog.subtitle
            elide: Text.ElideMiddle
            color: Kirigami.Theme.disabledTextColor
        }
        Kirigami.SearchField {
            id: search
            Layout.fillWidth: true
            onTextChanged: dialog.filter()
            onAccepted: {
                if (model.count)
                    dialog.current = model.get(0).code;
                dialog.accept();
            }
        }
        QQC2.ScrollView {
            Layout.fillWidth: true
            Layout.fillHeight: true
            Layout.preferredHeight: Kirigami.Units.gridUnit * 18
            ListView {
                id: list
                model: model
                clip: true
                delegate: QQC2.ItemDelegate {
                    required property string code
                    required property string name
                    required property string en
                    required property bool proofing
                    width: ListView.view.width
                    highlighted: code === dialog.current
                    text: name
                    contentItem: ColumnLayout {
                        spacing: 0
                        QQC2.Label {
                            text: name
                            color: parent.parent.highlighted ? Kirigami.Theme.highlightedTextColor : Kirigami.Theme.textColor
                        }
                        QQC2.Label {
                            text: en + (proofing ? "" : " · no proofing")
                            font: Kirigami.Theme.smallFont
                            color: parent.parent.highlighted ? Kirigami.Theme.highlightedTextColor : Kirigami.Theme.disabledTextColor
                            opacity: parent.parent.highlighted ? 0.8 : 1
                        }
                    }
                    onClicked: dialog.current = code
                    onDoubleClicked: {
                        dialog.current = code;
                        dialog.accept();
                    }
                }
            }
        }
    }
}
