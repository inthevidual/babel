import QtCore
import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Dialogs
import org.kde.kirigami as Kirigami
import se.cptjanst.babel

Kirigami.ApplicationWindow {
    id: root

    title: pageStack.currentItem === editor && editor.projectName ? editor.projectName : ""
    width: 1500
    height: 950
    minimumWidth: 860
    minimumHeight: 560

    pageStack.initialPage: projects
    pageStack.globalToolBar.style: Kirigami.ApplicationHeaderStyle.ToolBar
    pageStack.globalToolBar.showNavigationButtons: Kirigami.ApplicationHeaderStyle.ShowBackButton
    pageStack.columnView.columnResizeMode: Kirigami.ColumnView.SingleColumn

    property string lastLang: "sv-SE"

    function inStack(item) {
        for (let i = 0; i < pageStack.depth; i++)
            if (pageStack.get(i) === item)
                return true;
        return false;
    }

    function openProject(id, name) {
        if (!inStack(editor))
            pageStack.push(editor);
        editor.load(id, name);
    }

    function showProjects() {
        while (pageStack.depth > 1)
            pageStack.pop();
    }

    // A .docx to translate: ask for the language, then make it a translation.
    function importFile(url) {
        pendingImport.original = url;
        pendingImport.translated = "";
        languages.ask("Translate into…", decodeURIComponent(url.toString().split("/").pop()), root.lastLang, "Start Translating");
    }

    function pairFiles(original, translated) {
        pendingImport.original = original;
        pendingImport.translated = translated;
        languages.ask("Translate into…", decodeURIComponent(original.toString().split("/").pop()), root.lastLang, "Pair and Open");
    }

    QtObject {
        id: pendingImport
        property url original
        property url translated
        // The language dialog asks for a new translation's language, or the open one's.
        property bool changing: false
    }

    // The editor closes its translation (saving) when it leaves the stack.
    Connections {
        target: root.pageStack
        function onCurrentItemChanged() {
            if (!root.inStack(editor))
                editor.close();
        }
    }

    ProjectsPage {
        id: projects
        onOpenRequested: (id, name) => root.openProject(id, name)
        onImportRequested: openDialog.open()
        onFileDropped: url => root.importFile(url)
        onPairRequested: pairDialog.open()
        onExportRequested: id => {
            exportDialog.projectId = id;
            exportDialog.selectedFile = "file://" + StandardPaths.writableLocation(StandardPaths.DocumentsLocation).toString().replace(/^file:\/\//, "") + "/" + Backend.exportName(id, 0);
            exportDialog.open();
        }
        onAccountRequested: account.open()
    }

    NativePage {
        id: editor
        visible: false
        onExportRequested: id => projects.exportRequested(id)
        onVersionsRequested: id => versions.openFor(id)
        onLanguageRequested: current => {
            pendingImport.changing = true;
            languages.ask("Target language", "Used for spelling and grammar, and set as the language of the exported document.", current, "Change");
        }
        onSettingsRequested: settings.openNow()
        onHelpRequested: help.open()
    }

    LanguageDialog {
        id: languages
        onChosen: code => {
            if (pendingImport.changing) {
                pendingImport.changing = false;
                Backend.setLanguage(code);
                editor.languageChanged(code);
                return;
            }
            root.lastLang = code;
            const id = pendingImport.translated.toString()
                ? Backend.pairFiles(pendingImport.original, pendingImport.translated, code)
                : Backend.importFile(pendingImport.original, code);
            if (id)
                root.openProject(id, decodeURIComponent(pendingImport.original.toString().split("/").pop()).replace(/\.docx$/i, ""));
        }
        onRejected: pendingImport.changing = false
    }

    VersionsDialog {
        id: versions
        onRestored: editor.reloadSoon()
    }

    SettingsDialog {
        id: settings
        onChanged: s => editor.settingsChanged(s)
    }

    HelpDialog {
        id: help
    }

    PairDialog {
        id: pairDialog
        onPair: (o, t) => root.pairFiles(o, t)
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

    FileDialog {
        id: exportDialog
        property string projectId
        title: "Export translation"
        fileMode: FileDialog.SaveFile
        nameFilters: ["Word documents (*.docx)"]
        onAccepted: Backend.exportProject(projectId, "", selectedFile)
    }

    Shortcut {
        sequences: [StandardKey.Open]
        onActivated: openDialog.open()
    }

    Settings {
        property alias lastLang: root.lastLang
    }

    // babel-desktop [--lang=CODE] FILE.docx | --project=ID: open a document
    // (from the file manager; with --lang without asking for the language),
    // or a translation by its id. --native-type=TEXT is a test hook that
    // types into the editor without a keyboard.
    Component.onCompleted: {
        const args = Qt.application.arguments.slice(1);
        const lang = (args.find(a => a.startsWith("--lang=")) || "").slice(7);
        const typed = args.find(a => a.startsWith("--native-type="));
        for (const arg of args) {
            if (arg.startsWith("--project=")) {
                root.openProject(arg.slice(10), "");
            } else if (arg.toLowerCase().endsWith(".docx")) {
                const url = arg.startsWith("file:") ? arg : "file://" + arg;
                if (lang) {
                    const id = Backend.importFile(url, lang);
                    if (id)
                        root.openProject(id, decodeURIComponent(url.split("/").pop()).replace(/\.docx$/i, ""));
                } else {
                    root.importFile(url);
                }
            }
        }
        if (typed && root.inStack(editor))
            editor.typeTest(typed.slice(14));
    }

    Connections {
        target: Backend
        function onMessage(text, error) {
            root.showPassiveNotification(text, error ? "long" : "short");
        }
    }
}
