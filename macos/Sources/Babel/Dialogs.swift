// The dialogs: the target language, versions, the account, continuing from
// a translated file, a proofing issue, settings and help.

import AppKit
import SwiftUI

// ── Target language ───────────────────────────────────────────────────────

struct LanguagePicker: View {
    let title: String
    let subtitle: String
    let languages: [Language]
    @State var current: String
    let ok: String
    let close: () -> Void
    let chosen: (String) -> Void
    @State private var query = ""

    init(title: String, subtitle: String, languages: [Language], current: String, ok: String, close: @escaping () -> Void, chosen: @escaping (String) -> Void) {
        self.title = title
        self.subtitle = subtitle
        self.languages = languages
        _current = State(initialValue: current)
        self.ok = ok
        self.close = close
        self.chosen = chosen
    }

    private var filtered: [Language] {
        let q = query.trimmingCharacters(in: .whitespaces).lowercased()
        return q.isEmpty ? languages : languages.filter { "\($0.name) \($0.en) \($0.code)".lowercased().contains(q) }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            Text(title).font(.headline)
            if !subtitle.isEmpty {
                Text(subtitle).foregroundStyle(.secondary).lineLimit(3).truncationMode(.middle)
            }
            TextField("Search", text: $query)
                .textFieldStyle(.roundedBorder)
                .onSubmit {
                    if let first = filtered.first { current = first.code }
                    accept()
                }
            ScrollViewReader { proxy in
                List(filtered, selection: Binding(get: { current }, set: { if let c = $0 { current = c } })) { l in
                    VStack(alignment: .leading, spacing: 1) {
                        Text(l.name)
                        Text(l.en + (l.proofing ? "" : " · no proofing")).font(.caption).foregroundStyle(.secondary)
                    }
                    .tag(l.code)
                    .contentShape(Rectangle())
                    .onTapGesture(count: 2) {
                        current = l.code
                        accept()
                    }
                }
                .onAppear { proxy.scrollTo(current, anchor: .center) }
            }
            .frame(height: 340)
            HStack {
                Spacer()
                Button("Cancel", role: .cancel) { close() }
                    .keyboardShortcut(.cancelAction)
                Button(ok) { accept() }
                    .keyboardShortcut(.defaultAction)
                    .disabled(current.isEmpty)
            }
        }
        .padding(20)
        .frame(width: 380)
    }

    private func accept() {
        guard !current.isEmpty else { return }
        close()
        chosen(current)
    }
}

// ── Versions ──────────────────────────────────────────────────────────────

struct VersionsView: View {
    let projectId: String
    let close: () -> Void
    let restored: () -> Void
    @State private var name = ""
    @State private var versions: [JSON] = []

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Versions").font(.headline)
            Text("Kept every few minutes of work, at the start of each session, on export and before a restore; or named by you.")
                .font(.callout).foregroundStyle(.secondary)
            HStack {
                TextField("Name for a version of how it is now", text: $name)
                    .textFieldStyle(.roundedBorder)
                    .onSubmit(keep)
                Button("Keep Version", action: keep)
            }
            List {
                ForEach(versions.indices, id: \.self) { i in
                    let v = versions[i]
                    let st = v.obj("stats") ?? [:]
                    HStack {
                        VStack(alignment: .leading, spacing: 1) {
                            Text(Fmt.when(v.num("time"))).fontWeight(.medium)
                            Text("\(v.str("reason") == "auto" ? "automatic" : v.str("reason")) · \(Fmt.percentDone(st)) % · \(Fmt.number(st.num("tgtChars"))) characters")
                                .font(.caption).foregroundStyle(.secondary)
                        }
                        Spacer()
                        Button {
                            AppModel.shared.export(projectId, sid: v.str("sid"), versionTime: v.num("time"))
                        } label: { Image(systemName: "square.and.arrow.up") }
                            .buttonStyle(.borderless)
                            .help("Export this version as .docx")
                        Button("Restore…") { restore(v) }
                    }
                    .padding(.vertical, 2)
                }
            }
            .overlay {
                if versions.isEmpty {
                    ContentUnavailableView("No Versions Yet", systemImage: "clock", description: Text("The first one is made after a few minutes of work."))
                }
            }
            .frame(height: 360)
            HStack {
                Spacer()
                Button("Close") { close() }.keyboardShortcut(.defaultAction)
            }
        }
        .padding(20)
        .frame(width: 560)
        .onAppear(perform: reload)
    }

    private func reload() {
        versions = Core.list("versions", ["id": projectId])
    }

    private func keep() {
        AppModel.report(Core.object("addVersion", ["name": name]))
        name = ""
        reload()
    }

    private func restore(_ v: JSON) {
        let alert = NSAlert()
        alert.messageText = "Restore this version?"
        alert.informativeText = "The translation goes back to how it was on \(Fmt.when(v.num("time"))). How it is now is kept as a version first."
        alert.addButton(withTitle: "Restore")
        alert.addButton(withTitle: "Cancel")
        guard alert.runModal() == .alertFirstButtonReturn else { return }
        AppModel.report(Core.object("restore", ["id": projectId, "sid": v.str("sid")]))
        close()
        restored()
    }
}

// ── Account ───────────────────────────────────────────────────────────────

struct AccountView: View {
    @Bindable var model: AppModel
    let close: () -> Void
    @State private var server = ""
    @State private var user = ""
    @State private var password = ""

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            Text("Babel Account").font(.headline)
            if model.signedIn {
                Text("Signed in as **\(model.syncUser)** on \(model.syncServer). Translations on this Mac and on the server are kept in step; edits to the same paragraph on two devices keep the latest, and the other is saved as a version.")
                    .fixedSize(horizontal: false, vertical: true)
            } else {
                Text("Sign in to a Babel server to back up your translations and work on them from other computers and the web. Without an account, everything stays on this Mac.")
                    .fixedSize(horizontal: false, vertical: true)
                Form {
                    TextField("Server:", text: $server, prompt: Text("https://babel.example.org"))
                    TextField("User name:", text: $user)
                    SecureField("Password:", text: $password)
                        .onSubmit(signIn)
                }
            }
            if !model.syncStatus.isEmpty {
                Text(model.syncStatus).font(.callout).foregroundStyle(.secondary)
            }
            HStack {
                Spacer()
                Button("Close") { close() }.keyboardShortcut(.cancelAction)
                if model.signedIn {
                    Button("Sync Now") { model.syncNow() }.disabled(model.syncing)
                    Button("Sign Out") { model.signOut() }
                } else {
                    Button("Sign In", action: signIn)
                        .keyboardShortcut(.defaultAction)
                        .disabled(server.trimmingCharacters(in: .whitespaces).isEmpty || user.trimmingCharacters(in: .whitespaces).isEmpty || password.isEmpty)
                }
            }
        }
        .padding(20)
        .frame(width: 440)
        .onAppear {
            server = model.syncServer
            user = model.syncUser
        }
        .onChange(of: model.signedIn) { _, now in
            if now { close() }
        }
    }

    private func signIn() {
        guard !server.isEmpty, !user.isEmpty, !password.isEmpty else { return }
        model.signIn(server: server, user: user, password: password)
        password = ""
    }
}

// ── Continue from a translated file ───────────────────────────────────────

struct PairView: View {
    let close: () -> Void
    @State private var original: URL?
    @State private var translated: URL?

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            Text("Continue from a Translated File").font(.headline)
            Text("If you have the original and a translation previously exported from Babel (or with the same paragraph structure), pair them to pick up where you left off.")
                .fixedSize(horizontal: false, vertical: true)
            Grid(alignment: .leading, horizontalSpacing: 10, verticalSpacing: 8) {
                GridRow {
                    Text("Original:").gridColumnAlignment(.trailing)
                    Text(original?.lastPathComponent ?? "—").lineLimit(1).truncationMode(.middle).frame(width: 220, alignment: .leading)
                    Button("Choose…") { choose { original = $0 } }
                }
                GridRow {
                    Text("Translation:")
                    Text(translated?.lastPathComponent ?? "—").lineLimit(1).truncationMode(.middle).frame(width: 220, alignment: .leading)
                    Button("Choose…") { choose { translated = $0 } }
                }
            }
            HStack {
                Spacer()
                Button("Cancel", role: .cancel) { close() }.keyboardShortcut(.cancelAction)
                Button("Pair and Open") {
                    guard let original, let translated else { return }
                    close()
                    AppModel.shared.pair(original: original, translated: translated)
                }
                .keyboardShortcut(.defaultAction)
                .disabled(original == nil || translated == nil)
            }
        }
        .padding(20)
        .frame(width: 460)
    }

    private func choose(_ done: @escaping (URL) -> Void) {
        let p = NSOpenPanel()
        p.allowedContentTypes = [.docx]
        if p.runModal() == .OK, let url = p.url { done(url) }
    }
}

// ── A proofing issue ──────────────────────────────────────────────────────

struct IssueView: View {
    let issue: Issue
    let apply: (String) -> Void
    let ignore: () -> Void
    let addWord: () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text(issue.short.isEmpty ? (issue.kind == "spelling" ? "Spelling" : "Grammar") : issue.short)
                .font(.headline)
            Text(issue.message).fixedSize(horizontal: false, vertical: true)
            if !issue.replacements.isEmpty {
                FlowButtons(items: issue.replacements, apply: apply)
            }
            HStack {
                Button("Ignore", action: ignore)
                if issue.kind == "spelling" {
                    Button("Add to Dictionary", action: addWord)
                }
            }
            .buttonStyle(.link)
        }
        .padding(14)
        .frame(width: 300, alignment: .leading)
    }
}

/// Suggestions as buttons, wrapping.
struct FlowButtons: View {
    let items: [String]
    let apply: (String) -> Void

    var body: some View {
        let rows = stride(from: 0, to: min(items.count, 12), by: 3).map { Array(items[$0..<min($0 + 3, items.count, 12)]) }
        VStack(alignment: .leading, spacing: 6) {
            ForEach(rows.indices, id: \.self) { r in
                HStack(spacing: 6) {
                    ForEach(rows[r], id: \.self) { s in
                        Button(s.isEmpty ? "(remove)" : s) { apply(s) }
                    }
                }
            }
        }
    }
}

// ── Settings ──────────────────────────────────────────────────────────────

/// The web editor's settings, kept on this Mac: proofing with LanguageTool
/// (picky mode, a Premium account or own server), the personal dictionary,
/// and the marks beside paragraphs. Applied when changed.
struct SettingsView: View {
    @State private var proof = true
    @State private var picky = false
    @State private var endpoint = ""
    @State private var user = ""
    @State private var key = ""
    @State private var marks = true
    @State private var words = ""
    @State private var loaded = false

    var body: some View {
        Form {
            Section("Spelling and grammar") {
                Toggle("Check edited paragraphs with LanguageTool", isOn: $proof)
                Toggle("Picky mode (more style suggestions)", isOn: $picky).disabled(!proof)
                Text("The free service checks up to about 20 paragraphs a minute. A LanguageTool Premium account or your own server lifts that limit. Only the text of edited paragraphs is sent.")
                    .font(.callout).foregroundStyle(.secondary)
                TextField("Server:", text: $endpoint, prompt: Text("https://api.languagetool.org")).disabled(!proof)
                TextField("Premium user:", text: $user).disabled(!proof)
                SecureField("API key:", text: $key).disabled(!proof)
            }
            Section("Personal dictionary, one word a line") {
                TextEditor(text: $words)
                    .font(.body)
                    .frame(height: 110)
            }
            Section {
                Toggle("Mark translated and done paragraphs in the margin", isOn: $marks)
            }
        }
        .formStyle(.grouped)
        .frame(width: 500)
        .fixedSize(horizontal: false, vertical: true)
        .onAppear(perform: load)
        .onDisappear(perform: save)
        .onChange(of: proof) { save() }
        .onChange(of: picky) { save() }
        .onChange(of: marks) { save() }
        .onSubmit(save)
    }

    private func load() {
        let s = Core.object("settings") ?? [:]
        proof = s["proof"] == nil ? true : s.bool("proof")
        picky = s.bool("picky")
        endpoint = s.str("endpoint")
        user = s.str("user")
        key = s.str("key")
        marks = s["marks"] == nil ? true : s.bool("marks")
        words = ((s["dictionary"] as? [String]) ?? []).joined(separator: "\n")
        loaded = true
    }

    private func save() {
        guard loaded else { return }
        let s: JSON = [
            "proof": proof,
            "picky": picky,
            "endpoint": endpoint.trimmingCharacters(in: .whitespaces).isEmpty ? "https://api.languagetool.org" : endpoint.trimmingCharacters(in: .whitespaces),
            "user": user.trimmingCharacters(in: .whitespaces),
            "key": key.trimmingCharacters(in: .whitespaces),
            "marks": marks,
            "dictionary": words.split(separator: "\n").map { $0.trimmingCharacters(in: .whitespaces) }.filter { !$0.isEmpty },
        ]
        Core.call("setSettings", s)
        if let editor = MainWindowController.shared?.editor {
            editor.showMarks = marks
            editor.proofing = proof && editor.editing
            editor.target.needsDisplay = true
            editor.refreshIssues()
        }
    }
}

enum SettingsWindow {
    private static var window: NSWindow?

    static func show() {
        if window == nil {
            let w = NSWindow(contentViewController: NSHostingController(rootView: SettingsView()))
            w.title = "Settings"
            w.styleMask = [.titled, .closable]
            w.isReleasedWhenClosed = false
            w.center()
            window = w
        }
        window?.makeKeyAndOrderFront(nil)
    }
}

// ── Help ──────────────────────────────────────────────────────────────────

struct HelpView: View {
    let keys: [(String, String)] = [
        ("Click, drag", "Place the caret, select (within a paragraph)"),
        ("Double-click, triple-click", "Select a word, the paragraph"),
        ("Shift-arrows, ⌘←, ⌘→", "Select; start, end of the line"),
        ("Return", "Next paragraph"),
        ("Shift-Return", "Line break"),
        ("⌘Return", "Mark the paragraph as done (or not) and go to the next"),
        ("⌘B, ⌘I, ⌘U", "Bold, italic, underline the selection"),
        ("⌘Z, ⇧⌘Z", "Undo, redo"),
        ("⌘C, ⌘X, ⌘V", "Copy, cut, paste as plain text"),
        ("⌘K", "Make the selection a link (to the link you copied in the original, or an address you copied)"),
        ("⌘A", "Select the paragraph"),
        ("Control-click", "In the translation: revert, copy the original. In the original: copy, look up, copy a link"),
        ("Click in the original", "Go to the paragraph’s translation"),
        ("⌘T", "Text view (segments side by side) or the pages. In the text view: Return next segment, ⌘Return done and next, ⌘1 insert Claude’s translation, Up/Down at a segment’s edge to the one before/after"),
        ("Shift-click", "In the text view: pick the segments from the one you’re in to this one (⌘-click: one more or one less; Shift-Up/Down at a segment’s edge: one more). Then ⌘1, ⌘Return or a Control-click act on them all; Esc puts them down"),
        ("File › Attach Claude’s Translation…", "A segment file Claude translated (the babel-translate skill); then Control-click a paragraph › Insert Claude’s Translation"),
    ]

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 14) {
                Text("Type over the translation paragraph by paragraph. The exported .docx is the original file with the words changed: same styles, numbering, tables, pictures, headers, footers, footnotes and fields. Paragraphs are never split or merged.")
                    .fixedSize(horizontal: false, vertical: true)
                Grid(alignment: .topLeading, horizontalSpacing: 14, verticalSpacing: 6) {
                    ForEach(keys.indices, id: \.self) { i in
                        GridRow {
                            Text(keys[i].0).fontWeight(.semibold)
                            Text(keys[i].1).fixedSize(horizontal: false, vertical: true)
                        }
                    }
                }
                Text("Your translations are kept on this Mac and, when you are signed in, synced with your Babel account. Pages are laid out by Babel’s own engine, built on WordCraft (MIT or Apache-2.0).")
                    .font(.callout).foregroundStyle(.secondary)
                    .fixedSize(horizontal: false, vertical: true)
            }
            .padding(20)
        }
        .frame(width: 620, height: 560)
    }
}

enum HelpWindow {
    private static var window: NSWindow?

    static func show() {
        if window == nil {
            let w = NSWindow(contentViewController: NSHostingController(rootView: HelpView()))
            w.title = "Babel Keys and Help"
            w.styleMask = [.titled, .closable, .resizable]
            w.isReleasedWhenClosed = false
            w.center()
            window = w
        }
        window?.makeKeyAndOrderFront(nil)
    }
}
