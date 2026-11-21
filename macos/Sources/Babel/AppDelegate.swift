// Start-up, the menu bar, documents opened from the Finder, and the command
// line: Babel [--lang=CODE] FILE.docx (with --lang without asking for the
// language) or --project=ID.

import AppKit

final class AppDelegate: NSObject, NSApplicationDelegate {
    private var pendingFiles: [URL] = []
    private var main: MainWindowController?
    private var launched = false

    func applicationWillFinishLaunching(_ notification: Notification) {
        do {
            try Core.start()
        } catch {
            let alert = NSAlert(error: error)
            alert.runModal()
            exit(1)
        }
        NSApp.mainMenu = Menus.make()
        NSWindow.allowsAutomaticWindowTabbing = false
    }

    func applicationDidFinishLaunching(_ notification: Notification) {
        let model = AppModel.shared
        model.syncState(Core.object("account") ?? [:])
        Core.connect()
        model.reload()
        let wc = MainWindowController()
        main = wc
        wc.showWindow(nil)
        NSApp.activate(ignoringOtherApps: true)
        launched = true

        let args = Array(CommandLine.arguments.dropFirst())
        let lang = args.first { $0.hasPrefix("--lang=") }.map { String($0.dropFirst(7)) } ?? ""
        for arg in args {
            if arg.hasPrefix("--project=") {
                model.selection = String(arg.dropFirst(10))
            } else if arg.lowercased().hasSuffix(".docx") {
                let url = URL(fileURLWithPath: arg)
                if lang.isEmpty { model.openDocument(url) } else { model.make(original: url, translated: nil, lang: lang) }
            }
        }
        for url in pendingFiles { model.openDocument(url) }
        pendingFiles = []
        TestDriver.startIfAsked()
    }

    // Documents dropped on the Dock icon or opened from the Finder.
    func application(_ application: NSApplication, open urls: [URL]) {
        let docs = urls.filter { $0.pathExtension.lowercased() == "docx" }
        guard launched else {
            pendingFiles += docs
            return
        }
        if let url = docs.first { AppModel.shared.openDocument(url) }
    }

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool { true }

    func applicationWillTerminate(_ notification: Notification) {
        Core.call("close")
    }

    func applicationSupportsSecureRestorableState(_ app: NSApplication) -> Bool { true }

    // ── App-wide actions ─────────────────────────────────────────────────

    @objc func openDocument(_ sender: Any?) { AppModel.shared.openDocument() }

    @objc func continueFromTranslatedFile(_ sender: Any?) { Sheets.pair() }

    @objc func showAccount(_ sender: Any?) { Sheets.account() }

    @objc func syncNow(_ sender: Any?) {
        if AppModel.shared.signedIn { AppModel.shared.syncNow() } else { Sheets.account() }
    }

    @objc func showSettings(_ sender: Any?) { SettingsWindow.show() }

    @objc func showHelp(_ sender: Any?) { HelpWindow.show() }
}

extension AppDelegate: NSMenuItemValidation {
    func validateMenuItem(_ item: NSMenuItem) -> Bool {
        if item.action == #selector(syncNow(_:)) {
            item.title = AppModel.shared.signedIn ? "Sync Now" : "Sign In to Sync…"
            return !AppModel.shared.syncing
        }
        return true
    }
}

/// The menu bar. Editing actions go to the first responder (the pages, a
/// segment, or a text field), and on to the editor (MainWindowController).
enum Menus {
    static func make() -> NSMenu {
        let main = NSMenu()

        let appMenu = NSMenu(title: "Babel")
        appMenu.addItem(withTitle: "About Babel", action: #selector(NSApplication.orderFrontStandardAboutPanel(_:)), keyEquivalent: "")
        appMenu.addItem(.separator())
        appMenu.addItem(withTitle: "Settings…", action: #selector(AppDelegate.showSettings(_:)), keyEquivalent: ",")
        appMenu.addItem(withTitle: "Account…", action: #selector(AppDelegate.showAccount(_:)), keyEquivalent: "")
        appMenu.addItem(.separator())
        let services = NSMenu(title: "Services")
        appMenu.addItem(withTitle: "Services", action: nil, keyEquivalent: "").submenu = services
        NSApp.servicesMenu = services
        appMenu.addItem(.separator())
        appMenu.addItem(withTitle: "Hide Babel", action: #selector(NSApplication.hide(_:)), keyEquivalent: "h")
        appMenu.addItem(withTitle: "Hide Others", action: #selector(NSApplication.hideOtherApplications(_:)), keyEquivalent: "h").keyEquivalentModifierMask = [.command, .option]
        appMenu.addItem(withTitle: "Show All", action: #selector(NSApplication.unhideAllApplications(_:)), keyEquivalent: "")
        appMenu.addItem(.separator())
        appMenu.addItem(withTitle: "Quit Babel", action: #selector(NSApplication.terminate(_:)), keyEquivalent: "q")
        add(appMenu, to: main)

        let file = NSMenu(title: "File")
        file.addItem(withTitle: "Open Document…", action: #selector(AppDelegate.openDocument(_:)), keyEquivalent: "o")
        file.addItem(withTitle: "Continue from a Translated File…", action: #selector(AppDelegate.continueFromTranslatedFile(_:)), keyEquivalent: "")
        file.addItem(.separator())
        file.addItem(withTitle: "Export .docx…", action: #selector(EditorController.exportDocument(_:)), keyEquivalent: "e")
        file.addItem(withTitle: "Versions…", action: #selector(EditorController.showVersions(_:)), keyEquivalent: "")
        file.addItem(withTitle: "Target Language…", action: #selector(EditorController.changeLanguage(_:)), keyEquivalent: "")
        file.addItem(.separator())
        file.addItem(withTitle: "Export Segments for Claude…", action: #selector(EditorController.exportSegments(_:)), keyEquivalent: "")
        file.addItem(withTitle: "Attach Claude’s Translation…", action: #selector(EditorController.attachClaude(_:)), keyEquivalent: "")
        file.addItem(withTitle: "Remove Claude’s Translation", action: #selector(EditorController.removeClaude(_:)), keyEquivalent: "")
        file.addItem(.separator())
        file.addItem(withTitle: "Sync Now", action: #selector(AppDelegate.syncNow(_:)), keyEquivalent: "")
        file.addItem(.separator())
        file.addItem(withTitle: "Close Window", action: #selector(NSWindow.performClose(_:)), keyEquivalent: "w")
        add(file, to: main)

        let edit = NSMenu(title: "Edit")
        edit.addItem(withTitle: "Undo", action: Selector(("undo:")), keyEquivalent: "z")
        edit.addItem(withTitle: "Redo", action: Selector(("redo:")), keyEquivalent: "Z")
        edit.addItem(.separator())
        edit.addItem(withTitle: "Cut", action: #selector(NSText.cut(_:)), keyEquivalent: "x")
        edit.addItem(withTitle: "Copy", action: #selector(NSText.copy(_:)), keyEquivalent: "c")
        edit.addItem(withTitle: "Paste", action: #selector(NSText.paste(_:)), keyEquivalent: "v")
        edit.addItem(withTitle: "Paste Link", action: #selector(EditorController.pasteLink(_:)), keyEquivalent: "k")
        edit.addItem(withTitle: "Select All", action: #selector(NSText.selectAll(_:)), keyEquivalent: "a")
        edit.addItem(.separator())
        let find = NSMenu(title: "Find")
        find.addItem(withTitle: "Find…", action: #selector(EditorController.findSegments(_:)), keyEquivalent: "f")
        edit.addItem(withTitle: "Find", action: nil, keyEquivalent: "").submenu = find
        add(edit, to: main)

        let format = NSMenu(title: "Format")
        format.addItem(withTitle: "Bold", action: #selector(EditorController.toggleBold(_:)), keyEquivalent: "b")
        format.addItem(withTitle: "Italic", action: #selector(EditorController.toggleItalic(_:)), keyEquivalent: "i")
        format.addItem(withTitle: "Underline", action: #selector(EditorController.toggleUnderline(_:)), keyEquivalent: "u")
        add(format, to: main)

        let tr = NSMenu(title: "Translation")
        let done = tr.addItem(withTitle: "Mark as Done", action: #selector(EditorController.markDone(_:)), keyEquivalent: "\r")
        done.keyEquivalentModifierMask = [.command]
        tr.addItem(withTitle: "Revert Paragraph to the Original", action: #selector(EditorController.revertParagraph(_:)), keyEquivalent: "")
        tr.addItem(withTitle: "Copy the Original Text", action: #selector(EditorController.copyOriginal(_:)), keyEquivalent: "")
        tr.addItem(withTitle: "Insert Claude’s Translation", action: #selector(EditorController.insertClaude(_:)), keyEquivalent: "1")
        add(tr, to: main)

        let view = NSMenu(title: "View")
        view.addItem(withTitle: "Text View", action: #selector(EditorController.toggleTextView(_:)), keyEquivalent: "t")
        view.addItem(withTitle: "Mark Translated and Done Paragraphs", action: #selector(EditorController.toggleMarks(_:)), keyEquivalent: "")
        view.addItem(.separator())
        view.addItem(withTitle: "Actual Size", action: #selector(EditorController.actualSize(_:)), keyEquivalent: "0")
        view.addItem(withTitle: "Zoom In", action: #selector(EditorController.zoomIn(_:)), keyEquivalent: "+")
        view.addItem(withTitle: "Zoom Out", action: #selector(EditorController.zoomOut(_:)), keyEquivalent: "-")
        view.addItem(.separator())
        view.addItem(withTitle: "Show Sidebar", action: #selector(NSSplitViewController.toggleSidebar(_:)), keyEquivalent: "s").keyEquivalentModifierMask = [.command, .control]
        view.addItem(withTitle: "Enter Full Screen", action: #selector(NSWindow.toggleFullScreen(_:)), keyEquivalent: "f").keyEquivalentModifierMask = [.command, .control]
        add(view, to: main)

        let window = NSMenu(title: "Window")
        window.addItem(withTitle: "Minimize", action: #selector(NSWindow.performMiniaturize(_:)), keyEquivalent: "m")
        window.addItem(withTitle: "Zoom", action: #selector(NSWindow.performZoom(_:)), keyEquivalent: "")
        add(window, to: main)
        NSApp.windowsMenu = window

        let help = NSMenu(title: "Help")
        help.addItem(withTitle: "Babel Keys and Help", action: #selector(AppDelegate.showHelp(_:)), keyEquivalent: "?")
        add(help, to: main)
        NSApp.helpMenu = help

        return main
    }

    private static func add(_ menu: NSMenu, to main: NSMenu) {
        main.addItem(withTitle: menu.title, action: nil, keyEquivalent: "").submenu = menu
    }
}
