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
        pageStack.currentIndex = pageStack.depth - 1;
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

    // The back arrow only moves to the list and leaves the editor in the stack:
    // take it out, and close its translation (saving), when the list comes back.
    Connections {
        target: root.pageStack
        function onCurrentItemChanged() {
            if (root.pageStack.currentItem === projects && root.inStack(editor))
                Qt.callLater(root.leaveEditor);
        }
    }

    function leaveEditor() {
        if (pageStack.currentItem !== projects)
            return;
        editor.close();
        pageStack.pop(projects);
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

    // UI test driver: with BABEL_TEST_SCRIPT set, runs its steps through real
    // input events and logs the state, then quits (test/ui/).
    Timer {
        id: driver
        property var steps: []
        property int at: 0
        interval: 150
        repeat: true
        function state() {
            return JSON.stringify({
                depth: root.pageStack.depth,
                editing: editor.editing,
                project: editor.projectName,
                caret: editor.caret ? { page: editor.caret.page, x: Math.round(editor.caret.x) } : null,
                para: editor.paragraph ? editor.paragraph.pid : null,
                selected: Backend.testSelected(),
                sourceSelected: Backend.testSourceSelected(),
                targetY: Math.round(editor.targetY()),
                chars: editor.stats ? editor.stats.tgtChars : null,
            });
        }
        onTriggered: {
            if (at >= steps.length) {
                Qt.quit();
                return;
            }
            const s = steps[at++];
            const clicks = n => {
                for (let i = 0; i < n; i++) {
                    Backend.testMouse(1, s.x, s.y, s.mods || 0);
                    if (i === 1)
                        Backend.testMouse(3, s.x, s.y, s.mods || 0);
                    Backend.testMouse(2, s.x, s.y, s.mods || 0);
                }
            };
            if (s.wait)
                interval = s.wait;
            else
                interval = 150;
            if (s.click)
                clicks(s.click);
            if (s.drag) {
                const [x0, y0, x1, y1] = s.drag;
                Backend.testMouse(0, x0, y0, 0);
                Backend.testMouse(1, x0, y0, 0);
                for (let i = 1; i <= 12; i++)
                    Backend.testMouse(0, x0 + (x1 - x0) * i / 12, y0 + (y1 - y0) * i / 12, 0);
                Backend.testMouse(2, x1, y1, 0);
            }
            if (s.key !== undefined)
                Backend.testKey(s.key, s.text || "", s.mods || 0);
            if (s.grab)
                Backend.testGrab(s.grab);
            if (s.log)
                console.warn("UITEST", s.log, state());
            if (s.expect) {
                const st = JSON.parse(state());
                for (const [k, want] of Object.entries(s.expect)) {
                    const got = k === "selectedStarts" ? st.selected.startsWith(want)
                        : k === "sourceStarts" ? st.sourceSelected.startsWith(want)
                        : k === "selectedSome" ? (st.selected.length > 0) === want
                        : k === "charsBelow" ? st.chars < want
                        : k === "scrolled" ? st.targetY !== want
                        : st[k] === want;
                    const ok = got === true || (k in st && got === true);
                    console.warn(ok ? "UITEST ok  " : "UITEST FAIL", s.name || "", k, JSON.stringify(want), ok ? "" : state());
                }
            }
        }
        Component.onCompleted: {
            const script = Backend.testScript();
            if (script) {
                steps = JSON.parse(script);
                start();
            }
        }
    }

    Connections {
        target: Backend
        function onMessage(text, error) {
            root.showPassiveNotification(text, error ? "long" : "short");
        }
    }
}
