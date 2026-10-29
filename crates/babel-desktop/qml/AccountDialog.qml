import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import se.cptjanst.babel

Kirigami.Dialog {
    id: dialog

    title: "Babel Account"
    preferredWidth: Kirigami.Units.gridUnit * 24
    padding: Kirigami.Units.largeSpacing
    standardButtons: Kirigami.Dialog.NoButton

    customFooterActions: Backend.signedIn ? [signOutAction, closeAction] : [signInAction, closeAction]

    Kirigami.Action {
        id: signInAction
        text: "Sign In"
        icon.name: "go-next"
        enabled: server.text.trim() !== "" && user.text.trim() !== "" && password.text !== ""
        onTriggered: {
            Backend.signIn(server.text, user.text, password.text);
            password.text = "";
        }
    }
    Kirigami.Action {
        id: signOutAction
        text: "Sign Out"
        icon.name: "system-log-out"
        onTriggered: Backend.signOut()
    }
    Kirigami.Action {
        id: closeAction
        text: "Close"
        icon.name: "dialog-close"
        onTriggered: dialog.close()
    }

    Connections {
        target: Backend
        function onSignedInChanged() {
            if (Backend.signedIn)
                dialog.close();
        }
    }

    ColumnLayout {
        spacing: Kirigami.Units.largeSpacing

        QQC2.Label {
            Layout.fillWidth: true
            wrapMode: Text.Wrap
            text: Backend.signedIn
                ? "Signed in as <b>" + Backend.syncUser + "</b> on " + Backend.syncServer + ". Translations on this computer and on the server are kept in step; edits to the same paragraph on two devices keep the latest, and the other is saved as a version."
                : "Sign in to a Babel server to back up your translations and work on them from other computers and the web. Without an account, everything stays on this computer."
        }

        Kirigami.FormLayout {
            visible: !Backend.signedIn
            Layout.fillWidth: true
            QQC2.TextField {
                id: server
                Kirigami.FormData.label: "Server:"
                placeholderText: "https://babel.example.org"
                text: Backend.syncServer
                inputMethodHints: Qt.ImhUrlCharactersOnly | Qt.ImhNoAutoUppercase
            }
            QQC2.TextField {
                id: user
                Kirigami.FormData.label: "User name:"
                text: Backend.syncUser
                inputMethodHints: Qt.ImhNoAutoUppercase
            }
            Kirigami.PasswordField {
                id: password
                Kirigami.FormData.label: "Password:"
                onAccepted: if (signInAction.enabled) signInAction.trigger()
            }
        }

        QQC2.Label {
            Layout.fillWidth: true
            visible: Backend.syncStatus !== ""
            wrapMode: Text.Wrap
            color: Kirigami.Theme.disabledTextColor
            text: Backend.syncStatus
        }
    }
}
