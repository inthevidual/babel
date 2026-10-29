import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import se.cptjanst.babel

// The web editor's settings, kept on this computer: proofing with
// LanguageTool (picky mode, a Premium account or own server), the personal
// dictionary, and the marks beside paragraphs.
Kirigami.Dialog {
    id: dialog

    signal changed(var settings)

    title: "Settings"
    preferredWidth: Kirigami.Units.gridUnit * 26
    padding: Kirigami.Units.largeSpacing
    standardButtons: Kirigami.Dialog.Ok | Kirigami.Dialog.Cancel

    function openNow() {
        const s = JSON.parse(Backend.settings());
        proof.checked = s.proof;
        picky.checked = s.picky;
        endpoint.text = s.endpoint;
        user.text = s.user;
        key.text = s.key;
        marks.checked = s.marks;
        words.text = (s.dictionary || []).join("\n");
        open();
    }

    onAccepted: {
        const s = {
            proof: proof.checked,
            picky: picky.checked,
            endpoint: endpoint.text.trim() || "https://api.languagetool.org",
            user: user.text.trim(),
            key: key.text.trim(),
            marks: marks.checked,
            dictionary: words.text.split("\n").map(w => w.trim()).filter(w => w),
        };
        Backend.setSettings(JSON.stringify(s));
        changed(s);
    }

    ColumnLayout {
        spacing: Kirigami.Units.smallSpacing
        Kirigami.Heading { level: 4; text: "Spelling and grammar" }
        QQC2.CheckBox { id: proof; text: "Check edited paragraphs with LanguageTool" }
        QQC2.CheckBox { id: picky; text: "Picky mode (more style suggestions)"; enabled: proof.checked }
        QQC2.Label {
            Layout.fillWidth: true
            wrapMode: Text.Wrap
            color: Kirigami.Theme.disabledTextColor
            text: "The free service checks up to about 20 paragraphs a minute. A LanguageTool Premium account or your own server lifts that limit. Only the text of edited paragraphs is sent."
        }
        Kirigami.FormLayout {
            Layout.fillWidth: true
            enabled: proof.checked
            QQC2.TextField { id: endpoint; Kirigami.FormData.label: "Server:"; placeholderText: "https://api.languagetool.org" }
            QQC2.TextField { id: user; Kirigami.FormData.label: "Premium user:" }
            Kirigami.PasswordField { id: key; Kirigami.FormData.label: "API key:" }
        }
        QQC2.Label { text: "Personal dictionary, one word a line:" }
        QQC2.ScrollView {
            Layout.fillWidth: true
            Layout.preferredHeight: Kirigami.Units.gridUnit * 6
            QQC2.TextArea { id: words; wrapMode: TextEdit.NoWrap }
        }
        Kirigami.Separator { Layout.fillWidth: true }
        QQC2.CheckBox { id: marks; text: "Mark translated and done paragraphs in the margin" }
    }
}
