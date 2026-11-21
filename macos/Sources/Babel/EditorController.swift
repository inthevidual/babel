// The editor: both documents as Babel's own layout engine lays them out,
// side by side and scrolling together, or the text view (segments in rows).
// The editing is babel-editor's (Rust, shared with the browser and the Qt
// app): every action is a call whose answer is the new state, taken here.
//
// The translation: click, drag, double- or triple-click to place the caret
// or select (within a paragraph), type (input methods included), arrows with
// Shift to select, Shift-Return for a line break, Return for the next
// paragraph, ⌘Return to mark it done, ⌘Z/⇧⌘Z, ⌘B/I/U, ⌘A, ⌘C/X/V, ⌘K to
// paste a link; Control-click for the rest. Spelling and grammar are checked
// as you go. Saved a moment after typing stops.
//
// The original: a click goes to the translation's paragraph; select to copy
// or look up; Control-click a link to copy it.

import AppKit
import SwiftUI
import UniformTypeIdentifiers

final class EditorController: NSViewController, NSMenuItemValidation {
    var onChange: (() -> Void)?

    private(set) var projectId = ""
    private(set) var projectName = ""
    private(set) var targetLang = ""
    private(set) var editing = false

    var caret: Caret?
    var selection: [PageRect] = []
    var sourceSelection: [PageRect] = []
    var styles = (b: false, i: false, u: false)
    var paragraph: JSON?
    /// Claude's translation attached: {matched, total}, or nil.
    var claude: JSON?
    var stats: JSON?
    var canUndo = false
    var canRedo = false
    var underlines: [PageRect] = []
    var marks: [PageRect] = []
    var proofStatus = ""
    var issueCount = 0
    var proofing = false
    var showMarks = true
    private(set) var unsaved = false

    var hasSelection: Bool { !selection.isEmpty }
    var paragraphDone: Bool { paragraph?.bool("done") ?? false }

    var zoom: CGFloat = {
        let z = UserDefaults.standard.double(forKey: "zoom")
        return z > 0 ? z : 1
    }() {
        didSet {
            UserDefaults.standard.set(zoom, forKey: "zoom")
            source.zoom = zoom
            target.zoom = zoom
        }
    }

    /// The text view (rows of segments) instead of the pages; remembered.
    private(set) var textView = UserDefaults.standard.bool(forKey: "textView")

    let source = PagesView(side: .source)
    let target = PagesView(side: .target)
    private let sourceScroll = NSScrollView()
    private let targetScroll = NSScrollView()
    private let pagesSplit = NSSplitView()
    let segments = SegmentsController()
    private let placeholder: NSHostingView<EditorPlaceholder> = {
        let v = NSHostingView(rootView: EditorPlaceholder(model: AppModel.shared))
        v.sizingOptions = []
        return v
    }()
    private let sourceStatus = NSTextField(labelWithString: "")
    private let targetStatus = NSTextField(labelWithString: "")
    private let statusBar = NSVisualEffectView()
    private var saveTimer: Timer?
    private var followingScroll = false
    private var popover: NSPopover?

    // ── Views ────────────────────────────────────────────────────────────

    override func loadView() {
        let root = NSView()
        source.editor = self
        target.editor = self
        source.zoom = zoom
        target.zoom = zoom
        segments.editor = self

        for (scroll, pages) in [(sourceScroll, source), (targetScroll, target)] {
            scroll.documentView = pages
            scroll.hasVerticalScroller = true
            scroll.hasHorizontalScroller = true
            scroll.autohidesScrollers = true
            scroll.drawsBackground = true
            scroll.backgroundColor = .underPageBackgroundColor
            scroll.borderType = .noBorder
            scroll.contentView.postsBoundsChangedNotifications = true
            NotificationCenter.default.addObserver(self, selector: #selector(scrolled(_:)), name: NSView.boundsDidChangeNotification, object: scroll.contentView)
            NotificationCenter.default.addObserver(self, selector: #selector(resized(_:)), name: NSView.frameDidChangeNotification, object: scroll.contentView)
        }
        pagesSplit.isVertical = true
        pagesSplit.dividerStyle = .thin
        pagesSplit.addArrangedSubview(sourceScroll)
        pagesSplit.addArrangedSubview(targetScroll)
        pagesSplit.delegate = self

        addChild(segments)
        statusBar.material = .titlebar
        statusBar.blendingMode = .withinWindow
        for l in [sourceStatus, targetStatus] {
            l.font = .systemFont(ofSize: NSFont.smallSystemFontSize)
            l.textColor = .secondaryLabelColor
            l.lineBreakMode = .byTruncatingTail
            l.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        }
        let line = NSBox()
        line.boxType = .separator

        for v in [pagesSplit, segments.view, placeholder, statusBar, line, sourceStatus, targetStatus] as [NSView] {
            v.translatesAutoresizingMaskIntoConstraints = false
        }
        root.addSubview(pagesSplit)
        root.addSubview(segments.view)
        root.addSubview(placeholder)
        root.addSubview(statusBar)
        statusBar.addSubview(line)
        statusBar.addSubview(sourceStatus)
        statusBar.addSubview(targetStatus)
        NSLayoutConstraint.activate([
            statusBar.leadingAnchor.constraint(equalTo: root.leadingAnchor),
            statusBar.trailingAnchor.constraint(equalTo: root.trailingAnchor),
            statusBar.bottomAnchor.constraint(equalTo: root.bottomAnchor),
            statusBar.heightAnchor.constraint(equalToConstant: 24),
            line.topAnchor.constraint(equalTo: statusBar.topAnchor),
            line.leadingAnchor.constraint(equalTo: statusBar.leadingAnchor),
            line.trailingAnchor.constraint(equalTo: statusBar.trailingAnchor),
            sourceStatus.leadingAnchor.constraint(equalTo: statusBar.leadingAnchor, constant: 12),
            sourceStatus.centerYAnchor.constraint(equalTo: statusBar.centerYAnchor),
            sourceStatus.trailingAnchor.constraint(lessThanOrEqualTo: statusBar.centerXAnchor, constant: -12),
            targetStatus.leadingAnchor.constraint(equalTo: statusBar.centerXAnchor, constant: 12),
            targetStatus.centerYAnchor.constraint(equalTo: statusBar.centerYAnchor),
            targetStatus.trailingAnchor.constraint(lessThanOrEqualTo: statusBar.trailingAnchor, constant: -12),
        ])
        for v in [pagesSplit, segments.view, placeholder] as [NSView] {
            NSLayoutConstraint.activate([
                v.leadingAnchor.constraint(equalTo: root.leadingAnchor),
                v.trailingAnchor.constraint(equalTo: root.trailingAnchor),
                v.topAnchor.constraint(equalTo: root.safeAreaLayoutGuide.topAnchor),
                v.bottomAnchor.constraint(equalTo: statusBar.topAnchor),
            ])
        }
        view = root
        NotificationCenter.default.addObserver(self, selector: #selector(event(_:)), name: .babelEvent, object: nil)
        showViews()
    }

    private func showViews() {
        placeholder.isHidden = editing
        pagesSplit.isHidden = !editing || textView
        segments.view.isHidden = !editing || !textView
        statusBar.isHidden = !editing
    }

    override func viewDidLayout() {
        super.viewDidLayout()
        // The panes start equal.
        if pagesSplit.frame.width > 0, sourceScroll.frame.width == 0 || abs(sourceScroll.frame.width - targetScroll.frame.width) > pagesSplit.frame.width * 0.9 {
            pagesSplit.setPosition(pagesSplit.frame.width / 2, ofDividerAt: 0)
        }
    }

    // The two documents scroll together.
    @objc private func scrolled(_ n: Notification) {
        guard !followingScroll, let clip = n.object as? NSClipView else { return }
        let other = clip === sourceScroll.contentView ? targetScroll : sourceScroll
        followingScroll = true
        var origin = other.contentView.bounds.origin
        origin.y = clip.bounds.origin.y
        other.contentView.scroll(to: origin)
        other.reflectScrolledClipView(other.contentView)
        followingScroll = false
        hidePopover()
    }

    @objc private func resized(_ n: Notification) {
        source.relayout()
        target.relayout()
    }

    // ── Opening and closing ──────────────────────────────────────────────

    func load(_ id: String) {
        if editing { Core.call("close") }
        resetState()
        projectId = id
        projectName = AppModel.shared.projects.first { $0.id == id }?.name ?? ""
        let opened = Core.object("open", ["id": id])
        guard let r = opened, r.has("source") else {
            AppModel.report(opened ?? ["error": "Could not open the translation."])
            editing = false
            showViews()
            onChange?()
            return
        }
        editing = true
        let settings = Core.object("settings") ?? [:]
        showMarks = settings["marks"] == nil ? true : settings.bool("marks")
        if projectName.isEmpty { projectName = r.str("name") }
        targetLang = r.str("targetLang")
        source.setPages(r.arr("source").map(PageInfo.init), fresh: true)
        target.setPages([], fresh: true)
        proofing = r.bool("proofing") && (settings["proof"] == nil || settings.bool("proof"))
        var first = r
        first["all"] = true
        answer(first)
        unsaved = false
        saveTimer?.invalidate()
        updateStatus()
        showViews()
        sourceScroll.contentView.scroll(to: .zero)
        targetScroll.contentView.scroll(to: .zero)
        if textView {
            Core.call("showPages", ["on": false])
            segments.load()
        } else {
            view.window?.makeFirstResponder(target)
        }
        onChange?()
    }

    func close() {
        if editing {
            saveTimer?.invalidate()
            Core.call("close")
        }
        editing = false
        unsaved = false
        projectId = ""
        resetState()
        hidePopover()
        source.setPages([], fresh: true)
        target.setPages([], fresh: true)
        segments.clear()
        showViews()
        onChange?()
    }

    private func resetState() {
        caret = nil
        selection = []
        sourceSelection = []
        styles = (false, false, false)
        paragraph = nil
        claude = nil
        stats = nil
        canUndo = false
        canRedo = false
        underlines = []
        marks = []
        proofStatus = ""
        issueCount = 0
    }

    func setTextView(_ on: Bool) {
        guard on != textView else { return }
        textView = on
        UserDefaults.standard.set(on, forKey: "textView")
        hidePopover()
        if editing {
            if on {
                Core.call("showPages", ["on": false])
                segments.load()
            } else {
                answer(Core.object("showPages", ["on": true]))
                view.window?.makeFirstResponder(target)
            }
        }
        showViews()
        onChange?()
    }

    // ── Answers ──────────────────────────────────────────────────────────

    /// Takes an answer from the editor: caret, selections, pages drawn again, marks.
    func answer(_ r: JSON?, fromClick: Bool = false) {
        guard let r else { return }
        if let pages = r["pages"] as? [JSON] {
            let infos = pages.map(PageInfo.init)
            if r.bool("all") { target.setPages(infos) } else { target.update(infos) }
            if !pages.isEmpty {
                markUnsaved()
                refreshIssues()
            }
        }
        if let m = r["marks"] as? [JSON] { marks = PageRect.list(m) }
        if r.has("error") { Toast.show(r.str("error"), error: false) }
        if r.has("message") { Toast.show(r.str("message"), error: false) }
        caret = Caret(r.obj("caret"))
        selection = PageRect.list(r.arr("selection"))
        sourceSelection = PageRect.list(r.arr("sourceSelection"))
        if let s = r.obj("styles") { styles = (s.bool("b"), s.bool("i"), s.bool("u")) }
        paragraph = r.obj("paragraph")
        if r.has("claude") { claude = r.obj("claude") }
        if let s = r.obj("stats") { stats = s }
        canUndo = r.bool("canUndo")
        canRedo = r.bool("canRedo")
        target.caretMoved()
        source.needsDisplay = true
        if !textView { showCaret() }
        if fromClick, let issue = Issue(r.obj("issue")) { showIssue(issue) } else { hidePopover() }
        if r.bool("changed") { markUnsaved() }
        if textView { segments.afterAnswer(r) }
        updateStatus()
        onChange?()
    }

    func refreshIssues() {
        guard editing, let r = Core.object("issues") else { return }
        underlines = PageRect.list(r.arr("underlines"))
        issueCount = r.int("count")
        proofStatus = r.str("status")
        segments.setIssues((r.obj("pids") ?? [:]).compactMapValues { ($0 as? NSNumber)?.intValue })
        target.needsDisplay = true
        updateStatus()
    }

    @objc private func event(_ n: Notification) {
        let e = n.userInfo as? JSON ?? [:]
        switch e.str("event") {
        case "issues": refreshIssues()
        case "external":
            // Saved elsewhere (sync, the web editor): shown, unless something here is
            // unsaved (then the save merges, and the merged state comes back with it).
            guard editing, e.str("id") == projectId, !unsaved else { return }
            if let r = Core.object("reload", ["id": projectId]) {
                answer(r)
                unsaved = false
                updateStatus()
            }
        default: break
        }
    }

    // ── Saving ───────────────────────────────────────────────────────────

    private func markUnsaved() {
        unsaved = true
        if saveTimer == nil || !(saveTimer?.isValid ?? false) {
            saveTimer = Timer.scheduledTimer(withTimeInterval: 1, repeats: false) { [weak self] _ in self?.save() }
        }
    }

    func save() {
        saveTimer?.invalidate()
        guard editing else { return }
        AppModel.report(Core.object("save"))
        unsaved = false
        updateStatus()
    }

    private func updateStatus() {
        guard let s = stats else {
            sourceStatus.stringValue = "Original"
            targetStatus.stringValue = "Translation"
            return
        }
        sourceStatus.stringValue = "Original · \(Fmt.number(s.num("srcChars"))) characters with spaces · \(s.int("srcPages")) pages"
        let d = s.num("srcChars") > 0 ? 100 * (s.num("tgtChars") - s.num("srcChars")) / s.num("srcChars") : 0
        var t = "Translation · \(Fmt.number(s.num("tgtChars"))) characters"
        if d != 0 { t += " " + (d > 0 ? "+" : "") + String(format: "%.1f %%", d) }
        t += " · \(Fmt.percentDone(s)) % done (\(s.int("done"))/\(s.int("total")))"
        t += unsaved ? " · saving…" : " · saved"
        if !proofStatus.isEmpty {
            t += " · " + proofStatus
        } else if proofing {
            t += " · " + (issueCount == 0 ? "no issues" : issueCount == 1 ? "1 issue" : "\(issueCount) issues")
        }
        if s.int("tgtPages") != s.int("srcPages") {
            t += " · \(s.int("tgtPages")) pages"
        }
        targetStatus.stringValue = t
    }

    // ── Editing ──────────────────────────────────────────────────────────

    /// A key for the editor; false if it did nothing with it.
    @discardableResult
    func key(_ name: String, text: String = "", extend: Bool = false) -> Bool {
        guard editing, caret != nil || name == "undo" || name == "redo" else { return false }
        guard let r = Core.object("key", ["key": name, "text": text, "extend": extend]) else { return false }
        answer(r)
        return true
    }

    func type(_ text: String) {
        guard editing, caret != nil, !text.isEmpty else { return }
        answer(Core.object("key", ["key": "text", "text": text]))
    }

    func copySelection(cut: Bool) {
        let text = Core.string("selectedText")
        guard !text.isEmpty else { return }
        Clipboard.set(text)
        if cut { key("delete") }
    }

    func pasteText() {
        guard let t = Clipboard.text(), !t.isEmpty else { return }
        type(t.replacingOccurrences(of: "\r\n", with: "\n"))
    }

    func copySource() {
        let t = Core.string("sourceSelectedText")
        if !t.isEmpty { Clipboard.set(t) }
    }

    /// Scrolls the translation so the caret is in view.
    func showCaret() {
        guard let rect = target.caretRect() else { return }
        target.scrollToVisible(rect.insetBy(dx: -20, dy: -40))
    }

    // ── Pages: mouse ─────────────────────────────────────────────────────

    func click(_ side: Core.Side, page: Int, at p: CGPoint, count: Int, extend: Bool, dragging: Bool = false) {
        guard editing else { return }
        let args: JSON = ["page": page, "x": p.x, "y": p.y, "extend": extend || dragging]
        let method: String
        switch (side, dragging ? 1 : count) {
        case (.target, 1): method = "click"
        case (.target, 2): method = "word"
        case (.target, _): method = "paragraph"
        case (.source, 1): method = "sourceClick"
        case (.source, 2): method = "sourceWord"
        case (.source, _): method = "sourceParagraph"
        }
        answer(Core.object(method, args), fromClick: side == .target && count == 1 && !extend && !dragging)
    }

    /// The translation's menu, after a Control-click at a point.
    func targetMenu(page: Int, at p: CGPoint) -> NSMenu {
        if selection.isEmpty { click(.target, page: page, at: p, count: 1, extend: false) }
        return contextMenu()
    }

    /// The translation's menu, for the caret or selection as it is.
    func contextMenu() -> NSMenu {
        let m = NSMenu()
        func add(_ title: String, _ action: Selector, _ enabled: Bool, symbol: String? = nil) {
            let i = m.addItem(withTitle: title, action: action, keyEquivalent: "")
            i.target = self
            i.isEnabled = enabled
            if let symbol { i.image = NSImage(systemSymbolName: symbol, accessibilityDescription: nil) }
        }
        m.autoenablesItems = false
        add("Cut", #selector(cut(_:)), hasSelection)
        add("Copy", #selector(copy(_:)), hasSelection)
        add("Paste as Plain Text", #selector(paste(_:)), true)
        add("Paste Link", #selector(pasteLink(_:)), hasSelection, symbol: "link")
        m.addItem(.separator())
        add(paragraphDone ? "Unmark as Done" : "Mark as Done", #selector(markDone(_:)), paragraph != nil, symbol: "checkmark.circle")
        add("Revert Paragraph to the Original", #selector(revertParagraph(_:)), paragraph?.bool("edited") ?? false, symbol: "arrow.uturn.backward")
        add("Copy the Original Text", #selector(copyOriginal(_:)), paragraph != nil)
        if claude != nil {
            add("Insert Claude’s Translation", #selector(insertClaude(_:)), paragraph?.bool("claude") ?? false, symbol: "text.insert")
        }
        m.addItem(.separator())
        add("Undo", #selector(undo(_:)), canUndo)
        add("Redo", #selector(redo(_:)), canRedo)
        add("Select Paragraph", #selector(selectAll(_:)), paragraph != nil)
        return m
    }

    /// The original's menu: what a Control-click found there.
    func sourceMenu(page: Int, at p: CGPoint, in view: NSView, viewPoint: CGPoint) -> NSMenu {
        let found = Core.object("sourceMenu", ["page": page, "x": p.x, "y": p.y]) ?? [:]
        let m = NSMenu()
        m.autoenablesItems = false
        let copy = m.addItem(withTitle: "Copy", action: #selector(copySourceItem(_:)), keyEquivalent: "")
        copy.target = self
        copy.isEnabled = !found.str("text").isEmpty
        let lookup = found.str("lookup")
        let define = m.addItem(withTitle: lookup.isEmpty ? "Look Up" : "Look Up “\(lookup)”", action: #selector(lookUpItem(_:)), keyEquivalent: "")
        define.target = self
        define.isEnabled = !lookup.isEmpty
        define.representedObject = [lookup, view, NSValue(point: viewPoint)] as [Any]
        let mw = m.addItem(withTitle: lookup.isEmpty ? "Look Up in Merriam-Webster" : "Look Up “\(lookup)” in Merriam-Webster", action: #selector(merriamWebster(_:)), keyEquivalent: "")
        mw.target = self
        mw.isEnabled = !lookup.isEmpty
        mw.representedObject = lookup
        m.addItem(.separator())
        let link = found.str("link")
        let li = m.addItem(withTitle: link.isEmpty ? "Copy Link" : "Copy Link  \(link)", action: #selector(copyLinkItem(_:)), keyEquivalent: "")
        li.target = self
        li.isEnabled = !link.isEmpty
        let para = m.addItem(withTitle: "Copy Paragraph Text", action: #selector(copyTextItem(_:)), keyEquivalent: "")
        para.target = self
        para.isEnabled = !found.str("paragraph").isEmpty
        para.representedObject = found.str("paragraph")
        return m
    }

    @objc private func copySourceItem(_ sender: Any?) { copySource() }

    @objc private func lookUpItem(_ sender: NSMenuItem) {
        guard let o = sender.representedObject as? [Any], let word = o[0] as? String, let v = o[1] as? NSView, let p = o[2] as? NSValue else { return }
        let attr = NSAttributedString(string: word, attributes: [.font: NSFont.systemFont(ofSize: 14 * zoom)])
        v.showDefinition(for: attr, at: p.pointValue)
    }

    @objc func merriamWebster(_ sender: NSMenuItem) {
        guard let word = sender.representedObject as? String, !word.isEmpty,
              let q = word.addingPercentEncoding(withAllowedCharacters: .urlPathAllowed.subtracting(CharacterSet(charactersIn: "/"))),
              let url = URL(string: "https://www.merriam-webster.com/dictionary/" + q) else { return }
        NSWorkspace.shared.open(url)
    }

    @objc private func copyLinkItem(_ sender: Any?) {
        if let link = Core.call("copySourceLink") as? String, !link.isEmpty {
            Clipboard.set(link)
            Toast.show("Link copied: \(link)", error: false)
        }
    }

    @objc func copyTextItem(_ sender: NSMenuItem) {
        if let t = sender.representedObject as? String, !t.isEmpty { Clipboard.set(t) }
    }

    // ── Proofing issues ──────────────────────────────────────────────────

    private func showIssue(_ issue: Issue) {
        hidePopover()
        guard let rect = target.caretRect() else { return }
        let p = NSPopover()
        p.behavior = .transient
        p.animates = false
        p.contentViewController = NSHostingController(rootView: IssueView(
            issue: issue,
            apply: { [weak self] value in
                self?.hidePopover()
                self?.answer(Core.object("apply", ["pid": issue.pid, "n": issue.n, "value": value]))
            },
            ignore: { [weak self] in
                self?.hidePopover()
                Core.call("ignore", ["pid": issue.pid, "n": issue.n])
                self?.refreshIssues()
            },
            addWord: { [weak self] in
                self?.hidePopover()
                Core.call("addWord", ["pid": issue.pid, "n": issue.n])
                self?.refreshIssues()
            }))
        p.show(relativeTo: rect, of: target, preferredEdge: .maxY)
        popover = p
    }

    func hidePopover() {
        popover?.performClose(nil)
        popover = nil
    }

    // ── Actions (menu bar, toolbar, menus) ───────────────────────────────

    @objc func undo(_ sender: Any?) { key("undo") }
    @objc func redo(_ sender: Any?) { key("redo") }
    @objc func cut(_ sender: Any?) { copySelection(cut: true) }
    @objc func copy(_ sender: Any?) {
        if hasSelection { copySelection(cut: false) } else { copySource() }
    }
    @objc func paste(_ sender: Any?) { pasteText() }
    @objc override func selectAll(_ sender: Any?) { key("selectAll") }
    @objc func toggleBold(_ sender: Any?) { key("bold") }
    @objc func toggleItalic(_ sender: Any?) { key("italic") }
    @objc func toggleUnderline(_ sender: Any?) { key("underline") }

    @objc func pasteLink(_ sender: Any?) {
        guard editing else { return }
        answer(Core.object("pasteLink", ["clipboard": Clipboard.text() ?? ""]))
    }

    @objc func markDone(_ sender: Any?) {
        if textView { segments.doneActive() } else { key("done") }
    }

    @objc func revertParagraph(_ sender: Any?) {
        guard editing else { return }
        answer(Core.object("revert"))
    }

    @objc func copyOriginal(_ sender: Any?) {
        if let t = Core.call("originalText") as? String { Clipboard.set(t) }
    }

    @objc func insertClaude(_ sender: Any?) {
        if textView { segments.insertClaudeActive() } else { answer(Core.object("insertClaude")) }
    }

    // The pages, or the text view's text, larger or smaller (each remembered).
    @objc func zoomIn(_ sender: Any?) { setZoom(textView ? Spans.zoom * 1.1 : zoom * 1.2) }
    @objc func zoomOut(_ sender: Any?) { setZoom(textView ? Spans.zoom / 1.1 : zoom / 1.2) }
    @objc func actualSize(_ sender: Any?) { setZoom(1) }

    private func setZoom(_ z: CGFloat) {
        if textView {
            Spans.zoom = min(2.5, max(0.7, z))
            segments.zoomChanged()
        } else {
            zoom = min(3, max(0.4, z))
        }
    }
    @objc func toggleTextView(_ sender: Any?) { setTextView(!textView) }

    @objc func toggleMarks(_ sender: Any?) {
        var s = Core.object("settings") ?? [:]
        showMarks.toggle()
        s["marks"] = showMarks
        Core.call("setSettings", s)
        target.needsDisplay = true
    }

    @objc func findSegments(_ sender: Any?) {
        guard editing else { return }
        setTextView(true)
        segments.focusSearch()
    }

    @objc func exportDocument(_ sender: Any?) {
        guard editing else { return }
        save()
        AppModel.shared.export(projectId)
    }

    @objc func showVersions(_ sender: Any?) {
        guard editing else { return }
        save()
        let id = projectId
        Sheets.versions(id) { [weak self] in
            // After a restore: carry on from it.
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.3) {
                guard let self, self.projectId == id, let r = Core.object("reload", ["id": id]) else { return }
                self.answer(r)
                self.unsaved = false
                self.updateStatus()
            }
        }
    }

    @objc func changeLanguage(_ sender: Any?) {
        guard editing else { return }
        Sheets.language(title: "Target language", subtitle: "Used for spelling and grammar, and set as the language of the exported document.", current: targetLang, ok: "Change") { [weak self] code in
            guard let self else { return }
            AppModel.report(Core.object("setLanguage", ["code": code]))
            self.targetLang = code
            self.refreshIssues()
            AppModel.shared.reload()
            self.onChange?()
        }
    }

    @objc func attachClaude(_ sender: Any?) {
        guard editing else { return }
        Panels.open(title: "Attach Claude’s translation (a segment file)", types: [.markdownText, .plainText]) { [weak self] url in
            self?.answer(Core.object("attachClaude", ["path": url.path]))
        }
    }

    @objc func exportSegments(_ sender: Any?) {
        guard editing else { return }
        Panels.save(title: "Export segments for Claude", name: Core.string("segmentsName", ["id": projectId]), types: [.markdownText]) { url in
            let r = Core.object("exportSegments", ["path": url.path]) ?? [:]
            if r.has("error") {
                Toast.show(r.str("error"), error: true)
            } else {
                Toast.show("Saved. Give the file to Claude with the Babel translate skill, then attach the translation it returns.", error: false)
            }
        }
    }

    @objc func removeClaude(_ sender: Any?) {
        guard editing else { return }
        answer(Core.object("removeClaude"))
    }

    func validateMenuItem(_ item: NSMenuItem) -> Bool {
        guard let action = item.action else { return false }
        switch action {
        case #selector(undo(_:)): return editing && canUndo
        case #selector(redo(_:)): return editing && canRedo
        case #selector(cut(_:)): return editing && hasSelection
        case #selector(copy(_:)): return editing && (hasSelection || !sourceSelection.isEmpty)
        case #selector(paste(_:)): return editing && caret != nil
        case #selector(selectAll(_:)): return editing && caret != nil
        case #selector(toggleBold(_:)):
            item.state = styles.b ? .on : .off
            return editing && (hasSelection || styles.b)
        case #selector(toggleItalic(_:)):
            item.state = styles.i ? .on : .off
            return editing && (hasSelection || styles.i)
        case #selector(toggleUnderline(_:)):
            item.state = styles.u ? .on : .off
            return editing && (hasSelection || styles.u)
        case #selector(pasteLink(_:)): return editing && hasSelection
        case #selector(markDone(_:)):
            item.title = paragraphDone ? "Unmark as Done" : "Mark as Done"
            return editing && paragraph != nil
        case #selector(revertParagraph(_:)): return editing && (paragraph?.bool("edited") ?? false)
        case #selector(copyOriginal(_:)): return editing && paragraph != nil
        case #selector(insertClaude(_:)): return editing && claude != nil && (paragraph?.bool("claude") ?? false)
        case #selector(removeClaude(_:)): return editing && claude != nil
        case #selector(attachClaude(_:)):
            item.title = claude == nil ? "Attach Claude’s Translation…" : "Replace Claude’s Translation…"
            return editing
        case #selector(toggleTextView(_:)):
            item.title = textView ? "Page View" : "Text View"
            return editing
        case #selector(toggleMarks(_:)):
            item.state = showMarks ? .on : .off
            return editing
        case #selector(zoomIn(_:)), #selector(zoomOut(_:)), #selector(actualSize(_:)): return editing
        default: return editing
        }
    }
}

extension EditorController: NSSplitViewDelegate {
    func splitView(_ splitView: NSSplitView, constrainMinCoordinate proposed: CGFloat, ofSubviewAt i: Int) -> CGFloat { 200 }
    func splitView(_ splitView: NSSplitView, constrainMaxCoordinate proposed: CGFloat, ofSubviewAt i: Int) -> CGFloat { splitView.frame.width - 200 }
}

enum Clipboard {
    static func set(_ text: String) {
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(text, forType: .string)
    }

    static func text() -> String? {
        NSPasteboard.general.string(forType: .string)
    }
}

/// What the editor shows with no translation open.
struct EditorPlaceholder: View {
    @Bindable var model: AppModel

    var body: some View {
        ContentUnavailableView {
            Label(model.projects.isEmpty ? "Translate a Word Document" : "No Translation Open", systemImage: "character.book.closed")
        } description: {
            Text("Open a .docx and type over it paragraph by paragraph. The exported file is the original with the words changed: same styles, numbering, tables, pictures, headers, footers, footnotes and fields.")
        } actions: {
            Button("Open Document…") { model.openDocument() }
                .keyboardShortcut("o")
            Button("Continue from a Translated File…") { Sheets.pair() }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(Color(nsColor: .underPageBackgroundColor))
    }
}
