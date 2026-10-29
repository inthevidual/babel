import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Dialogs
import org.kde.kirigami as Kirigami
import se.cptjanst.babel

Kirigami.ApplicationWindow {
    id: root

    title: editor.projectName && pageStack.currentItem === editor ? editor.projectName : ""
    width: 1500
    height: 950
    minimumWidth: 860
    minimumHeight: 560

    pageStack.initialPage: projects
    pageStack.globalToolBar.style: Kirigami.ApplicationHeaderStyle.ToolBar
    pageStack.globalToolBar.showNavigationButtons: Kirigami.ApplicationHeaderStyle.ShowBackButton
    pageStack.columnView.columnResizeMode: Kirigami.ColumnView.SingleColumn

    function openProject(id) {
        editor.openProject(id);
        showEditor();
    }

    function showEditor() {
        if (pageStack.currentItem !== editor)
            pageStack.push(editor);
    }

    function showProjects() {
        while (pageStack.depth > 1)
            pageStack.pop();
    }

    function importFile(url) {
        const id = Backend.stageFile(url);
        if (!id)
            return;
        const name = decodeURIComponent(url.toString().split("/").pop());
        editor.importFile(id, name);
        showEditor();
    }

    ProjectsPage {
        id: projects
        onOpenRequested: id => root.openProject(id)
        onImportRequested: openDialog.open()
        onFileDropped: url => root.importFile(url)
        onExportRequested: id => editor.downloadProject(id)
        onAccountRequested: account.open()
    }

    EditorPage {
        id: editor
        visible: false
        onClosed: root.showProjects()
        onOpened: root.showEditor()
    }

    AccountDialog {
        id: account
    }

    FileDialog {
        id: openDialog
        title: "Open a Word document"
        nameFilters: ["Word documents (*.docx)"]
        onAccepted: root.importFile(selectedFile)
    }

    Shortcut {
        sequences: [StandardKey.Open]
        onActivated: openDialog.open()
    }

    // babel-desktop [FILE.docx | --project=ID]: open a document (from the
    // file manager), or a translation by its id.
    Component.onCompleted: {
        for (const arg of Qt.application.arguments.slice(1)) {
            if (arg.startsWith("--project="))
                root.openProject(arg.slice(10));
            else if (arg.toLowerCase().endsWith(".docx"))
                root.importFile(arg.startsWith("file:") ? arg : "file://" + arg);
        }
    }

    Connections {
        target: Backend
        function onMessage(text, error) {
            root.showPassiveNotification(text, error ? "long" : "short");
        }
    }
}
