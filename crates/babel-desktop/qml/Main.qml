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

    function inStack(item) {
        for (let i = 0; i < pageStack.depth; i++)
            if (pageStack.get(i) === item)
                return true;
        return false;
    }

    function showEditor() {
        if (!inStack(editor))
            pageStack.push(editor);
    }

    // The editor closes its translation when it leaves the stack, not when
    // a page (the native view) is put on top of it.
    Connections {
        target: root.pageStack
        function onCurrentItemChanged() {
            if (!root.inStack(editor))
                editor.closeProject();
            if (!root.inStack(nativeView))
                nativeView.close();
        }
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
        onNativeRequested: (id, name) => {
            nativeView.load(id, name);
            pageStack.push(nativeView);
        }
    }

    NativePage {
        id: nativeView
        visible: false
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

    // babel-desktop [FILE.docx | --project=ID [--native]]: open a document
    // (from the file manager), or a translation by its id, optionally in the
    // native layout view.
    Component.onCompleted: {
        const args = Qt.application.arguments.slice(1);
        for (const arg of args) {
            if (arg.startsWith("--project=") && args.includes("--native")) {
                root.openProject(arg.slice(10));
                nativeView.load(arg.slice(10), "");
                pageStack.push(nativeView);
                // Test hook: type into the native editor without a keyboard.
                const typed = args.find(a => a.startsWith("--native-type="));
                if (typed)
                    nativeView.typeTest(typed.slice(14));
            } else if (arg.startsWith("--project="))
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
