// The app's state outside the editor: the translations, the account, the
// target languages, and the flows that make and export translations.

import AppKit
import Observation
import UniformTypeIdentifiers

struct Project: Identifiable, Equatable {
    let id: String
    let name: String
    let lang: String
    let done: Int
    let updated: Double

    init(_ j: JSON) {
        id = j.str("id")
        let n = j.str("name")
        name = n.isEmpty ? (j.str("fileName").isEmpty ? "Untitled" : j.str("fileName")) : n
        let ln = j.str("langName")
        lang = ln.isEmpty ? j.str("targetLang") : ln
        done = Fmt.percentDone(j.obj("stats"))
        updated = j.num("updatedAt")
    }
}

struct Language: Identifiable, Hashable {
    let code: String
    let name: String
    let en: String
    let proofing: Bool
    var id: String { code }
}

@Observable
final class AppModel {
    static let shared = AppModel()

    var projects: [Project] = []
    /// The translation open in the editor (the sidebar's selection).
    var selection: String? {
        didSet {
            if selection != oldValue { onSelect?(selection) }
        }
    }
    @ObservationIgnored var onSelect: ((String?) -> Void)?

    // The account.
    var signedIn = false
    var syncing = false
    var syncServer = ""
    var syncUser = ""
    var syncStatus = ""

    @ObservationIgnored lazy var languages: [Language] = Core.list("languages").map {
        Language(code: $0.str("code"), name: $0.str("name"), en: $0.str("en"), proofing: !($0["lt"] is NSNull || $0["lt"] == nil))
    }

    var lastLang: String {
        get { UserDefaults.standard.string(forKey: "lastLang") ?? "sv-SE" }
        set { UserDefaults.standard.set(newValue, forKey: "lastLang") }
    }

    private init() {
        NotificationCenter.default.addObserver(forName: .babelEvent, object: nil, queue: .main) { [weak self] n in
            self?.handle(n.userInfo as? JSON ?? [:])
        }
    }

    func reload() {
        projects = Core.list("projects").map(Project.init)
        if let s = selection, !projects.contains(where: { $0.id == s }) {
            selection = nil
        }
    }

    func syncState(_ j: JSON) {
        signedIn = j.bool("signedIn")
        syncing = j.bool("syncing")
        syncServer = j.str("server")
        syncUser = j.str("user")
        syncStatus = j.str("status")
    }

    private func handle(_ e: JSON) {
        switch e.str("event") {
        case "projects": reload()
        case "sync": syncState(e.obj("state") ?? [:])
        case "message": Toast.show(e.str("text"), error: e.bool("error"))
        default: break
        }
    }

    func languageName(_ code: String) -> String {
        languages.first { $0.code == code }?.name ?? code
    }

    // ── Making translations ───────────────────────────────────────────────

    /// Asks for a .docx (or takes `url`), then its target language, then opens it as a new translation.
    func openDocument(_ url: URL? = nil) {
        guard let url else {
            Panels.open(title: "Open a Word document", types: [.docx]) { self.openDocument($0) }
            return
        }
        guard url.pathExtension.lowercased() == "docx" else {
            Toast.show("“\(url.lastPathComponent)” is not a .docx file. Open it in Word and save it as .docx first.", error: true)
            return
        }
        Sheets.language(title: "Translate into…", subtitle: url.lastPathComponent, current: lastLang, ok: "Start Translating") { code in
            self.lastLang = code
            self.make(original: url, translated: nil, lang: code)
        }
    }

    /// Continue from a translated file: the original and an earlier translation of it.
    func pair(original: URL, translated: URL) {
        Sheets.language(title: "Translate into…", subtitle: original.lastPathComponent, current: lastLang, ok: "Pair and Open") { code in
            self.lastLang = code
            self.make(original: original, translated: translated, lang: code)
        }
    }

    @discardableResult
    func make(original: URL, translated: URL?, lang: String) -> Bool {
        var args: JSON = ["path": original.path, "lang": lang]
        if let translated { args["translated"] = translated.path }
        let r = Core.object("import", args) ?? [:]
        if r.has("error") {
            Toast.show(r.str("error"), error: true)
            return false
        }
        reload()
        selection = r.str("id")
        return true
    }

    // ── Exporting and deleting ────────────────────────────────────────────

    /// Exports the translated .docx (of a version, with `sid`).
    func export(_ id: String, sid: String? = nil, versionTime: Double = 0) {
        let name = Core.string("exportName", ["id": id, "versionTime": versionTime])
        Panels.save(title: sid == nil ? "Export translation" : "Export version", name: name, types: [.docx]) { url in
            var args: JSON = ["id": id, "path": url.path]
            if let sid { args["sid"] = sid }
            Self.report(Core.object("export", args))
        }
    }

    func delete(_ p: Project) {
        let alert = NSAlert()
        alert.messageText = "Delete “\(p.name)”?"
        alert.informativeText = "The translation and all its versions are deleted" + (signedIn ? ", here and on the server." : ". Export it first if you want to keep it.")
        alert.addButton(withTitle: "Delete").hasDestructiveAction = true
        alert.addButton(withTitle: "Cancel")
        Sheets.alert(alert) { response in
            guard response == .alertFirstButtonReturn else { return }
            if self.selection == p.id { self.selection = nil }
            Self.report(Core.object("delete", ["id": p.id]))
            self.reload()
        }
    }

    /// Shows what an answer says ({message} or {error}).
    static func report(_ r: JSON?) {
        guard let r else { return }
        if r.has("error") { Toast.show(r.str("error"), error: true) }
        else if r.has("message") { Toast.show(r.str("message"), error: false) }
    }

    // ── Account ───────────────────────────────────────────────────────────

    func signIn(server: String, user: String, password: String) {
        Core.call("signIn", ["server": server, "user": user, "password": password, "device": Host.current().localizedName ?? ""])
    }

    func signOut() { Core.call("signOut") }

    func syncNow() { Core.call("syncNow") }
}

extension UTType {
    static let docx = UTType("org.openxmlformats.wordprocessingml.document") ?? UTType(filenameExtension: "docx")!
    static let markdownText = UTType("net.daringfireball.markdown") ?? UTType(filenameExtension: "md") ?? .plainText
}

/// Open and save panels as sheets on the main window.
enum Panels {
    static func open(title: String, types: [UTType], _ done: @escaping (URL) -> Void) {
        let p = NSOpenPanel()
        p.message = title
        p.allowedContentTypes = types
        p.allowsMultipleSelection = false
        p.canChooseDirectories = false
        let finish: (NSApplication.ModalResponse) -> Void = { r in
            if r == .OK, let url = p.url { done(url) }
        }
        if let w = MainWindowController.shared?.window { p.beginSheetModal(for: w, completionHandler: finish) } else { finish(p.runModal()) }
    }

    static func save(title: String, name: String, types: [UTType], _ done: @escaping (URL) -> Void) {
        let p = NSSavePanel()
        p.message = title
        p.nameFieldStringValue = name
        p.allowedContentTypes = types
        p.canCreateDirectories = true
        if p.directoryURL == nil {
            p.directoryURL = FileManager.default.urls(for: .documentDirectory, in: .userDomainMask).first
        }
        let finish: (NSApplication.ModalResponse) -> Void = { r in
            if r == .OK, let url = p.url { done(url) }
        }
        if let w = MainWindowController.shared?.window { p.beginSheetModal(for: w, completionHandler: finish) } else { finish(p.runModal()) }
    }
}
