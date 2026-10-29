import QtCore
import QtQuick
import QtQuick.Dialogs
import QtWebEngine
import org.kde.kirigami as Kirigami
import se.cptjanst.babel

// The two document panels: the same editor as on the web, served by the
// app's local server, in a persistent profile so its recovery journal
// survives restarts.
Kirigami.Page {
    id: page

    property string projectName: ""
    property string projectId: ""
    property var queue: []
    property bool ready: false

    signal closed()
    signal opened()
    signal nativeRequested(string id, string name)

    actions: [
        Kirigami.Action {
            text: "Native Layout"
            icon.name: "view-preview"
            tooltip: "The original and the translation as Babel's own layout engine lays them out (a preview, read-only)"
            enabled: page.projectId !== ""
            // Save what was typed first: the native view reads what is stored.
            onTriggered: viewLoader.item.runJavaScript("__babel.flushAll(); __babel.save(); true", () => saveWait.start())
        }
    ]

    Timer {
        id: saveWait
        interval: 400
        onTriggered: page.nativeRequested(page.projectId, page.projectName)
    }

    title: projectName
    padding: 0

    // Back to the list: close the translation (it saves on closing).
    function closeProject() {
        if (ready)
            run("if (location.hash.startsWith('#p=')) location.hash = ''");
    }

    function run(js) {
        if (ready)
            viewLoader.item.runJavaScript(js);
        else
            queue.push(js);
    }

    function openProject(id) {
        run("location.hash = " + JSON.stringify("#p=" + id));
    }

    function importFile(stagedId, name) {
        run("babelDesktop.openStaged(" + JSON.stringify(stagedId) + ", " + JSON.stringify(name) + ")");
    }

    function downloadProject(id) {
        run("babelDesktop.download(" + JSON.stringify(id) + ")");
    }

    // The view is made once the profile exists: binding a view's profile to
    // a prototype that hasn't completed yet hands it no profile.
    WebEngineProfilePrototype {
        id: profileProto
        storageName: "babel"
        persistentStoragePath: Backend.dataDir + "/web"
        persistentCookiesPolicy: WebEngineProfile.ForcePersistentCookies
        Component.onCompleted: viewLoader.active = true
    }

    Loader {
        id: viewLoader
        anchors.fill: parent
        active: false
        sourceComponent: WebEngineView {
            id: web
            profile: profileProto.instance()
            url: Backend.editorUrl
            settings.javascriptCanAccessClipboard: true
            settings.javascriptCanPaste: true

            onLoadingChanged: request => {
                if (request.status !== WebEngineLoadingInfo.LoadSucceededStatus)
                    return;
                page.ready = true;
                const q = page.queue;
                page.queue = [];
                for (const js of q)
                    web.runJavaScript(js);
            }

            onJavaScriptConsoleMessage: (level, message, line, source) => {
                if (level === WebEngineView.ErrorMessageLevel)
                    console.warn("editor:", message, "(" + source + ":" + line + ")");
            }

            // The editor's own address changes with what it shows.
            onUrlChanged: {
                if (!page.ready)
                    return; // still loading; queued commands decide what it shows
                const hash = url.toString().split("#")[1] || "";
                page.projectId = hash.startsWith("p=") ? hash.slice(2) : "";
                if (hash.startsWith("p="))
                    page.opened();
                else if (hash !== "importing")
                    page.closed();
            }

            onTitleChanged: page.projectName = title.endsWith(" — Babel") ? title.slice(0, -8) : ""

            // Links out of the editor (dictionary look-ups, help) open in the browser.
            onNewWindowRequested: request => Qt.openUrlExternally(request.requestedUrl)
            onNavigationRequested: request => {
                const u = request.url.toString();
                if (!u.startsWith(Backend.origin) && !u.startsWith("about:") && !u.startsWith("blob:") && !u.startsWith("data:")) {
                    request.reject();
                    Qt.openUrlExternally(request.url);
                }
            }

            // The editor's own menu copies and pastes; nothing else is granted.
            onPermissionRequested: permission => {
                if (permission.permissionType === WebEnginePermission.PermissionType.ClipboardReadWrite
                        && permission.origin.toString().startsWith(Backend.origin))
                    permission.grant();
                else
                    permission.deny();
            }
        }
    }

    Connections {
        target: profileProto.instance()
        enabled: viewLoader.status === Loader.Ready
        function onDownloadRequested(download) {
            saveDialog.download = download;
            saveDialog.selectedFile = "file://" + saveDialog.folderPath() + "/" + download.downloadFileName;
            saveDialog.open();
        }
    }

    FileDialog {
        id: saveDialog
        property var download: null
        property string lastFolder: ""
        function folderPath() {
            return lastFolder || (StandardPaths.writableLocation(StandardPaths.DocumentsLocation).toString().replace(/^file:\/\//, ""));
        }
        title: "Export translation"
        fileMode: FileDialog.SaveFile
        nameFilters: ["Word documents (*.docx)"]
        onAccepted: {
            const path = decodeURIComponent(selectedFile.toString().replace(/^file:\/\//, ""));
            const slash = path.lastIndexOf("/");
            lastFolder = path.slice(0, slash);
            download.downloadDirectory = path.slice(0, slash);
            download.downloadFileName = path.slice(slash + 1);
            download.accept();
            download = null;
        }
        onRejected: {
            if (download)
                download.cancel();
            download = null;
        }
    }
}
