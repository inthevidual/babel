// The text view: the document as rows of segments, the original beside the
// translation (as in a CAT tool), with Claude's translation of the row being
// worked on in a panel. It edits the same translation as the pages, through
// the same editor: typing, formatting, undo, done marks and the menu actions
// are the page view's. Shift-click picks a range of rows, ⌘-click one more;
// then ⌘1, ⌘Return or a Control-click act on them all.

import AppKit
import SwiftUI

struct Row {
    var pid: String
    var n: Int
    var part: String
    var src: [JSON]
    var tgt: [JSON]
    var done: Bool
    var edited: Bool
    var claude: Bool
    var editable: Bool

    init(_ j: JSON) {
        pid = j.str("pid")
        n = j.int("n")
        part = j.str("part")
        src = j.arr("src")
        tgt = j.arr("tgt")
        done = j.bool("done")
        edited = j.bool("edited")
        claude = j.bool("claude")
        editable = j.bool("editable")
    }

    /// The text without chips (pictures, fields), as copied.
    static func text(_ spans: [JSON]) -> String {
        spans.filter { $0["chip"] == nil }.map { $0.str("t") }.joined()
    }

    /// What a text view holds for the spans (chips included).
    static func plain(_ spans: [JSON]) -> String {
        spans.map { $0.str("t") }.joined()
    }
}

enum Spans {
    /// The text view's zoom (1: the system's text size); remembered.
    static var zoom: CGFloat = {
        let z = UserDefaults.standard.double(forKey: "textZoom")
        return z > 0 ? z : 1
    }() {
        didSet { UserDefaults.standard.set(zoom, forKey: "textZoom") }
    }

    static var font: NSFont { NSFont.systemFont(ofSize: (13 * zoom).rounded()) }

    /// Spans as rich text: bold, italic, underline, links; numbered chips shaded.
    static func attributed(_ spans: [JSON]) -> NSAttributedString {
        let out = NSMutableAttributedString()
        let para = NSMutableParagraphStyle()
        para.lineSpacing = 2
        for s in spans {
            var attrs: [NSAttributedString.Key: Any] = [.font: font, .foregroundColor: NSColor.textColor, .paragraphStyle: para]
            if s["chip"] != nil {
                if s.int("chip") > 0 {
                    attrs[.backgroundColor] = NSColor.textColor.withAlphaComponent(0.12)
                    attrs[.foregroundColor] = NSColor.secondaryLabelColor
                    if !s.str("title").isEmpty { attrs[.toolTip] = s.str("title") }
                }
            } else {
                var f = font
                if s.bool("b") { f = NSFontManager.shared.convert(f, toHaveTrait: .boldFontMask) }
                if s.bool("i") { f = NSFontManager.shared.convert(f, toHaveTrait: .italicFontMask) }
                attrs[.font] = f
                if s.bool("u") || s.bool("l") { attrs[.underlineStyle] = NSUnderlineStyle.single.rawValue }
                if s.bool("l") { attrs[.foregroundColor] = NSColor.linkColor }
            }
            out.append(NSAttributedString(string: s.str("t"), attributes: attrs))
        }
        return out
    }
}

/// What the side panel shows.
@Observable
final class SegmentsPanel {
    var pickCount = 0
    var pickedClaude = 0
    var pickedDone = 0
    var pickedEdited = 0
    var segment = ""
    var claudeText: AttributedString?
    var attached = false
    var done = false
    var edited = false
    var hasActive = false

    @ObservationIgnored var act: (String) -> Void = { _ in }
}

final class SegmentsController: NSViewController {
    weak var editor: EditorController?

    private var all: [Row] = []
    private var at: [String: Int] = [:]
    private var shown: [Row] = []
    private var shownAt: [String: Int] = [:]
    private(set) var activePid = ""
    private var picked: Set<String> = []
    private var pickFrom = ""
    private var pickTo = ""
    /// While a row's own typing is answered: the row keeps its text and cursor.
    private var typing = false
    private var placing = false
    private var heights: [String: (CGFloat, CGFloat)] = [:]

    private let table = NSTableView()
    private let scroll = NSScrollView()
    private let search = NSSearchField()
    private let onlyOpen = NSButton(checkboxWithTitle: "Only segments not done", target: nil, action: nil)
    private let count = NSTextField(labelWithString: "")
    let panel = SegmentsPanel()

    private let numberCol = NSTableColumn(identifier: .init("n"))
    private let srcCol = NSTableColumn(identifier: .init("src"))
    private let tgtCol = NSTableColumn(identifier: .init("tgt"))

    override func loadView() {
        search.placeholderString = "Search the original and the translation"
        search.target = self
        search.action = #selector(filterChanged(_:))
        search.sendsSearchStringImmediately = false
        onlyOpen.target = self
        onlyOpen.action = #selector(filterChanged(_:))
        count.textColor = .secondaryLabelColor
        count.font = .systemFont(ofSize: NSFont.smallSystemFontSize)
        let bar = NSStackView(views: [search, onlyOpen, NSView(), count])
        bar.edgeInsets = NSEdgeInsets(top: 8, left: 12, bottom: 8, right: 12)
        bar.spacing = 12
        search.widthAnchor.constraint(equalToConstant: 280).isActive = true

        numberCol.title = ""
        numberCol.width = 44
        numberCol.minWidth = 44
        numberCol.maxWidth = 44
        numberCol.resizingMask = []
        srcCol.title = "Original"
        tgtCol.title = "Translation"
        for c in [srcCol, tgtCol] {
            c.minWidth = 160
            c.resizingMask = .autoresizingMask
        }
        table.addTableColumn(numberCol)
        table.addTableColumn(srcCol)
        table.addTableColumn(tgtCol)
        table.columnAutoresizingStyle = .uniformColumnAutoresizingStyle
        table.selectionHighlightStyle = .none
        table.intercellSpacing = NSSize(width: 0, height: 0)
        table.gridStyleMask = [.solidHorizontalGridLineMask]
        table.gridColor = NSColor.separatorColor.withAlphaComponent(0.4)
        table.style = .plain
        table.allowsColumnReordering = false
        table.dataSource = self
        table.delegate = self
        table.usesAlternatingRowBackgroundColors = false
        scroll.documentView = table
        scroll.hasVerticalScroller = true
        scroll.autohidesScrollers = true
        scroll.borderType = .noBorder
        scroll.contentView.postsFrameChangedNotifications = true
        NotificationCenter.default.addObserver(self, selector: #selector(widthChanged(_:)), name: NSView.frameDidChangeNotification, object: scroll.contentView)

        panel.act = { [weak self] in self?.panelAction($0) }
        let side = NSHostingView(rootView: SegmentsPanelView(panel: panel))

        let left = NSStackView(views: [bar, scroll])
        left.orientation = .vertical
        left.spacing = 0
        left.setHuggingPriority(.defaultLow, for: .horizontal)
        let line = NSBox()
        line.boxType = .separator
        let root = NSStackView(views: [left, line, side])
        root.orientation = .horizontal
        root.spacing = 0
        root.alignment = .top
        for v in [left, line, side] as [NSView] {
            v.heightAnchor.constraint(equalTo: root.heightAnchor).isActive = true
        }
        side.widthAnchor.constraint(equalToConstant: 280).isActive = true
        view = root
    }

    // ── Rows ─────────────────────────────────────────────────────────────

    func load() {
        all = Core.list("rows").map(Row.init)
        heights = [:]
        reindex()
        rebuild()
        activePid = ""
        if let first = shown.first {
            DispatchQueue.main.async { self.focusRow(first.pid, anchor: 0, caret: 0) }
        }
        updatePanel()
    }

    func clear() {
        all = []
        shown = []
        at = [:]
        shownAt = [:]
        activePid = ""
        picked = []
        table.reloadData()
    }

    private func reindex() {
        at = Dictionary(uniqueKeysWithValues: all.enumerated().map { ($1.pid, $0) })
    }

    private func matches(_ r: Row) -> Bool {
        if onlyOpen.state == .on && r.done { return false }
        let q = search.stringValue.trimmingCharacters(in: .whitespaces)
        if q.isEmpty { return true }
        return Row.text(r.src).localizedCaseInsensitiveContains(q) || Row.text(r.tgt).localizedCaseInsensitiveContains(q)
    }

    private func rebuild() {
        shown = all.filter(matches)
        shownAt = Dictionary(uniqueKeysWithValues: shown.enumerated().map { ($1.pid, $0) })
        table.reloadData()
        count.stringValue = shown.count == all.count ? "\(all.count) segments" : "\(shown.count) of \(all.count) segments"
    }

    @objc private func filterChanged(_ sender: Any?) {
        rebuild()
    }

    func focusSearch() {
        view.window?.makeFirstResponder(search)
    }

    /// The text size changed: every row shown again, at its new height.
    func zoomChanged() {
        heights = [:]
        table.enumerateAvailableRowViews { _, i in
            guard i < shown.count else { return }
            for col in [1, 2] {
                if let cell = table.view(atColumn: col, row: i, makeIfNecessary: false) as? TextCell {
                    cell.show(col == 1 ? shown[i].src : shown[i].tgt)
                }
            }
        }
        NSAnimationContext.runAnimationGroup { ctx in
            ctx.duration = 0
            table.noteHeightOfRows(withIndexesChanged: IndexSet(integersIn: 0..<shown.count))
        }
        updatePanel()
    }

    @objc private func widthChanged(_ n: Notification) {
        heights = [:]
        NSAnimationContext.runAnimationGroup { ctx in
            ctx.duration = 0
            table.noteHeightOfRows(withIndexesChanged: IndexSet(integersIn: 0..<shown.count))
        }
    }

    /// After any answer of the editor: rows changed are shown again, the cursor goes where the caret is.
    func afterAnswer(_ r: JSON) {
        guard let ch = Core.object("rowChanges") else { return }
        if ch.bool("all") {
            all = ch.arr("rows").map(Row.init)
            heights = [:]
            reindex()
            rebuild()
        } else {
            var changed = IndexSet()
            for j in ch.arr("rows") {
                let row = Row(j)
                if let i = at[row.pid] { all[i] = row }
                guard let i = shownAt[row.pid] else { continue }
                shown[i] = row
                heights[row.pid] = nil
                changed.insert(i)
                refreshCells(i)
            }
            if !changed.isEmpty {
                NSAnimationContext.runAnimationGroup { ctx in
                    ctx.duration = 0
                    table.noteHeightOfRows(withIndexesChanged: changed)
                }
            }
        }
        if let tc = r.obj("textCaret") {
            if !typing { focusRow(tc.str("pid"), anchor: tc.int("anchor"), caret: tc.int("at")) }
            setActive(tc.str("pid"))
        }
        typing = false
        updatePanel()
    }

    /// A row's cells show the row as it is now, unless the translation's
    /// field already holds it after its own typing (then the cursor and the
    /// input method stay as they are).
    private func refreshCells(_ i: Int) {
        let row = shown[i]
        if let cell = table.view(atColumn: 0, row: i, makeIfNecessary: false) as? NumberCell { cell.show(row) }
        if let cell = table.view(atColumn: 1, row: i, makeIfNecessary: false) as? TextCell { cell.show(row.src) }
        if let cell = table.view(atColumn: 2, row: i, makeIfNecessary: false) as? TextCell {
            let tv = cell.textView
            if typing && tv.window?.firstResponder === tv && tv.string == Row.plain(row.tgt) {
                cell.known = tv.string
            } else {
                cell.show(row.tgt)
            }
            tv.isEditable = row.editable
        }
        if let rv = table.rowView(atRow: i, makeIfNecessary: false) { rv.needsDisplay = true }
    }

    private func setActive(_ pid: String) {
        guard pid != activePid else { return }
        let old = activePid
        activePid = pid
        for p in [old, pid] { if let i = shownAt[p] { table.rowView(atRow: i, makeIfNecessary: false)?.needsDisplay = true } }
        updatePanel()
    }

    /// Puts the cursor (and selection: anchor to caret) in a row's translation.
    func focusRow(_ pid: String, anchor: Int, caret: Int) {
        guard let i = shownAt[pid] else { return }
        table.scrollRowToVisible(i)
        guard let cell = table.view(atColumn: 2, row: i, makeIfNecessary: true) as? TextCell else { return }
        let tv = cell.textView
        let len = (tv.string as NSString).length
        let a = min(anchor, len), c = min(caret, len)
        placing = true
        if tv.window?.firstResponder !== tv { tv.window?.makeFirstResponder(tv) }
        tv.setSelectedRange(NSRange(location: min(a, c), length: abs(c - a)))
        tv.scrollRangeToVisible(tv.selectedRange())
        placing = false
    }

    /// The next (1) or previous (-1) row, at its start or end.
    private func move(from pid: String, _ dir: Int) {
        guard let i = shownAt[pid] else { return }
        let j = i + dir
        guard j >= 0, j < shown.count else { return }
        let end = dir < 0 ? (Row.plain(shown[j].tgt) as NSString).length : 0
        focusRow(shown[j].pid, anchor: end, caret: end)
    }

    /// The editor's selection is the row's: then keys and menu actions act there.
    func sync(_ cell: TextCell) {
        guard let pid = cell.pid else { return }
        let r = cell.textView.selectedRange()
        editor?.answer(Core.object("textSelect", ["pid": pid, "anchor": r.location, "caret": r.location + r.length]))
    }

    // ── Typing in a row ──────────────────────────────────────────────────

    fileprivate func typed(_ cell: TextCell) {
        guard let pid = cell.pid else { return }
        let was = cell.known as NSString
        let now = cell.textView.string as NSString
        if now.isEqual(to: was as String) { return }
        // What changed: the part between what stayed at the start and at the end.
        var a = 0
        while a < was.length && a < now.length && was.character(at: a) == now.character(at: a) { a += 1 }
        var b = 0
        while b < was.length - a && b < now.length - a && was.character(at: was.length - 1 - b) == now.character(at: now.length - 1 - b) { b += 1 }
        let text = now.substring(with: NSRange(location: a, length: now.length - b - a))
        cell.known = now as String
        clearPicks()
        typing = true
        editor?.answer(Core.object("textEdit", ["pid": pid, "from": a, "to": was.length - b, "text": text]))
    }

    fileprivate func focused(_ cell: TextCell) {
        guard let pid = cell.pid, !placing else {
            if let pid = cell.pid { setActive(pid) }
            return
        }
        setActive(pid)
        sync(cell)
    }

    /// Keys in a row's translation; true if handled here.
    fileprivate func command(_ selector: Selector, in cell: TextCell) -> Bool {
        guard let pid = cell.pid else { return false }
        let tv = cell.textView
        let shift = NSApp.currentEvent?.modifierFlags.contains(.shift) ?? false
        switch NSStringFromSelector(selector) {
        case "cancelOperation:":
            if !picked.isEmpty { clearPicks(); return true }
            return false
        case "insertNewline:":
            if shift { return false }
            move(from: pid, 1)
            return true
        case "moveUp:":
            if tv.caretOnFirstLine { move(from: pid, -1); return true }
            return false
        case "moveDown:":
            if tv.caretOnLastLine { move(from: pid, 1); return true }
            return false
        case "moveUpAndModifySelection:":
            if (!picked.isEmpty || tv.selectedRange().length == 0) && tv.caretOnFirstLine { pickStep(from: pid, -1); return true }
            return false
        case "moveDownAndModifySelection:":
            if (!picked.isEmpty || tv.selectedRange().length == 0) && tv.caretOnLastLine { pickStep(from: pid, 1); return true }
            return false
        default:
            return false
        }
    }

    // ── Done and Claude, for a row or the picked rows ────────────────────

    /// Marks a row done (or not); marked, on to the next row.
    func done(_ pid: String, cell: TextCell?) {
        guard !pid.isEmpty, let editor else { return }
        if let cell { sync(cell) } else { editor.answer(Core.object("textSelect", ["pid": pid, "anchor": 0, "caret": 0])) }
        editor.key("done")
        if let i = at[pid], all[i].done { move(from: pid, 1) }
    }

    func insertClaude(_ pid: String, cell: TextCell?) {
        guard !pid.isEmpty, let editor else { return }
        if let cell { sync(cell) } else { editor.answer(Core.object("textSelect", ["pid": pid, "anchor": 0, "caret": 0])) }
        editor.answer(Core.object("insertClaude"))
    }

    /// ⌘Return wherever the keyboard is: the picked rows, or the active one.
    func doneActive() {
        if !picked.isEmpty { toggleDonePicked() } else { done(activePid, cell: activeCell()) }
    }

    func insertClaudeActive() {
        if !picked.isEmpty { rowsAction("claude") } else { insertClaude(activePid, cell: activeCell()) }
    }

    private func activeCell() -> TextCell? {
        guard let i = shownAt[activePid], let cell = table.view(atColumn: 2, row: i, makeIfNecessary: false) as? TextCell,
              cell.textView.window?.firstResponder === cell.textView else { return nil }
        return cell
    }

    // ── Picking rows ─────────────────────────────────────────────────────

    private func pickedPids() -> [String] {
        shown.map(\.pid).filter { picked.contains($0) }
    }

    private func setPicked(_ s: Set<String>) {
        picked = s
        table.enumerateAvailableRowViews { rv, _ in rv.needsDisplay = true }
        updatePanel()
    }

    func clearPicks() {
        if !picked.isEmpty { setPicked([]) }
        pickFrom = ""
        pickTo = ""
    }

    /// Shift-click: the rows from where the range starts (the row being worked on) to this one.
    fileprivate func pickRange(_ pid: String) {
        if pickFrom.isEmpty { pickFrom = activePid.isEmpty ? pid : activePid }
        pickTo = pid
        guard let b = shownAt[pid] else { return }
        let a = shownAt[pickFrom] ?? b
        setPicked(Set(shown[min(a, b)...max(a, b)].map(\.pid)))
    }

    /// ⌘-click: this row in, or out.
    fileprivate func pickToggle(_ pid: String) {
        var s = picked
        if s.isEmpty && !activePid.isEmpty && activePid != pid { s.insert(activePid) }
        if s.contains(pid) { s.remove(pid) } else { s.insert(pid) }
        pickFrom = pid
        pickTo = pid
        setPicked(s)
    }

    /// Shift-Up/Down at a row's edge: the range one row further.
    private func pickStep(from pid: String, _ dir: Int) {
        if pickFrom.isEmpty {
            pickFrom = pid
            pickTo = pid
        }
        guard let i = shownAt[pickTo] else { return }
        let j = i + dir
        guard j >= 0, j < shown.count else { return }
        pickRange(shown[j].pid)
        table.scrollRowToVisible(j)
    }

    /// An action on the picked rows: "claude", "done", "undone", "revert".
    func rowsAction(_ action: String) {
        let pids = pickedPids()
        guard !pids.isEmpty else { return }
        editor?.answer(Core.object("rowsAction", ["action": action, "pids": pids]))
    }

    private func toggleDonePicked() {
        let pids = pickedPids()
        rowsAction(pids.allSatisfy { at[$0].map { all[$0].done } ?? false } ? "undone" : "done")
    }

    private func pickedWith(_ key: KeyPath<Row, Bool>) -> Int {
        pickedPids().filter { at[$0].map { all[$0][keyPath: key] } ?? false }.count
    }

    private func copyPicked(source: Bool) {
        Clipboard.set(pickedPids().compactMap { at[$0].map { Row.text(source ? all[$0].src : all[$0].tgt) } }.joined(separator: "\n"))
    }

    fileprivate func isPicked(_ pid: String) -> Bool { picked.contains(pid) }
    fileprivate var pickCount: Int { picked.count }

    // ── Menus ────────────────────────────────────────────────────────────

    fileprivate func rowsMenu() -> NSMenu {
        let m = NSMenu()
        m.autoenablesItems = false
        m.addItem(withTitle: "\(picked.count) segments", action: nil, keyEquivalent: "").isEnabled = false
        m.addItem(.separator())
        func add(_ title: String, _ action: String, _ enabled: Bool) {
            let i = m.addItem(withTitle: title, action: #selector(rowsMenuItem(_:)), keyEquivalent: "")
            i.target = self
            i.representedObject = action
            i.isEnabled = enabled
        }
        add("Insert Claude’s Translation", "claude", editor?.claude != nil && pickedWith(\.claude) > 0)
        add("Mark as Done", "done", true)
        add("Unmark as Done", "undone", pickedWith(\.done) > 0)
        add("Revert to the Original", "revert", pickedWith(\.edited) > 0)
        m.addItem(.separator())
        add("Copy the Translations", "copyTgt", true)
        add("Copy the Originals", "copySrc", true)
        return m
    }

    @objc private func rowsMenuItem(_ sender: NSMenuItem) {
        switch sender.representedObject as? String {
        case "copyTgt": copyPicked(source: false)
        case "copySrc": copyPicked(source: true)
        case let a?: rowsAction(a)
        default: break
        }
    }

    /// The original's menu: copy, the whole paragraph, look up.
    fileprivate func sourceMenu(_ pid: String, selected: String, in view: NSView, at point: CGPoint) -> NSMenu {
        let sel = selected.replacingOccurrences(of: "\u{2028}", with: "\n").trimmingCharacters(in: .whitespacesAndNewlines)
        let lookup = !sel.isEmpty && sel.count < 60 && sel.split(whereSeparator: \.isWhitespace).count <= 5 ? sel : ""
        let m = NSMenu()
        m.autoenablesItems = false
        let copy = m.addItem(withTitle: "Copy", action: #selector(EditorController.copyTextItem(_:)), keyEquivalent: "")
        copy.target = editor
        copy.representedObject = sel
        copy.isEnabled = !sel.isEmpty
        let para = m.addItem(withTitle: "Copy Paragraph Text", action: #selector(EditorController.copyTextItem(_:)), keyEquivalent: "")
        para.target = editor
        para.representedObject = at[pid].map { Row.text(all[$0].src) } ?? ""
        m.addItem(.separator())
        let mw = m.addItem(withTitle: lookup.isEmpty ? "Look Up in Merriam-Webster" : "Look Up “\(lookup)” in Merriam-Webster", action: #selector(EditorController.merriamWebster(_:)), keyEquivalent: "")
        mw.target = editor
        mw.representedObject = lookup
        mw.isEnabled = !lookup.isEmpty
        return m
    }

    // ── The side panel ───────────────────────────────────────────────────

    private func updatePanel() {
        let p = panel
        p.pickCount = picked.count
        p.pickedClaude = pickedWith(\.claude)
        p.pickedDone = pickedWith(\.done)
        p.pickedEdited = pickedWith(\.edited)
        p.attached = editor?.claude != nil
        p.hasActive = !activePid.isEmpty
        p.segment = at[activePid].map { "Segment \(all[$0].n)" } ?? "Segment"
        p.done = editor?.paragraphDone ?? false
        p.edited = editor?.paragraph?.bool("edited") ?? false
        if !activePid.isEmpty, let spans = Core.call("claudeSpans", ["pid": activePid]) as? [JSON] {
            p.claudeText = try? AttributedString(Spans.attributed(spans), including: \.appKit)
        } else {
            p.claudeText = nil
        }
    }

    private func panelAction(_ a: String) {
        switch a {
        case "insert": insertClaude(activePid, cell: activeCell())
        case "done": done(activePid, cell: activeCell())
        case "revert":
            if let cell = activeCell() { sync(cell) }
            editor?.revertParagraph(nil)
        case "attach": editor?.attachClaude(nil)
        case "putDown": clearPicks()
        default: rowsAction(a)
        }
    }

    // ── Heights ──────────────────────────────────────────────────────────

    private static let measure: (NSTextStorage, NSLayoutManager, NSTextContainer) = {
        let storage = NSTextStorage()
        let layout = NSLayoutManager()
        let container = NSTextContainer(size: NSSize(width: 100, height: CGFloat.greatestFiniteMagnitude))
        container.lineFragmentPadding = 5
        layout.addTextContainer(container)
        storage.addLayoutManager(layout)
        return (storage, layout, container)
    }()

    private func textHeight(_ spans: [JSON], width: CGFloat) -> CGFloat {
        let (storage, layout, container) = Self.measure
        container.size = NSSize(width: max(40, width - 2 * TextCell.inset), height: .greatestFiniteMagnitude)
        storage.setAttributedString(Spans.attributed(spans))
        layout.ensureLayout(for: container)
        return max(layout.usedRect(for: container).height, Spans.font.boundingRectForFont.height)
    }

    fileprivate func height(_ row: Row) -> CGFloat {
        let ws = srcCol.width, wt = tgtCol.width
        if let (w, h) = heights[row.pid], w == ws + wt * 10_000 { return h }
        let h = max(textHeight(row.src, width: ws), textHeight(row.tgt, width: wt)) + 2 * TextCell.inset + 2
        heights[row.pid] = (ws + wt * 10_000, h)
        return h
    }
}

// ── The table ─────────────────────────────────────────────────────────────

extension SegmentsController: NSTableViewDataSource, NSTableViewDelegate {
    func numberOfRows(in tableView: NSTableView) -> Int { shown.count }

    func tableView(_ tableView: NSTableView, heightOfRow row: Int) -> CGFloat {
        row < shown.count ? height(shown[row]) : 40
    }

    func tableView(_ tableView: NSTableView, rowViewForRow row: Int) -> NSTableRowView? {
        let rv = tableView.makeView(withIdentifier: .init("row"), owner: nil) as? SegmentRowView ?? SegmentRowView()
        rv.identifier = .init("row")
        rv.controller = self
        rv.index = row
        return rv
    }

    func tableView(_ tableView: NSTableView, viewFor column: NSTableColumn?, row: Int) -> NSView? {
        guard row < shown.count, let column else { return nil }
        let r = shown[row]
        switch column.identifier.rawValue {
        case "n":
            let cell = tableView.makeView(withIdentifier: column.identifier, owner: nil) as? NumberCell ?? NumberCell()
            cell.identifier = column.identifier
            cell.show(r)
            return cell
        default:
            let isTarget = column.identifier.rawValue == "tgt"
            let cell = tableView.makeView(withIdentifier: column.identifier, owner: nil) as? TextCell ?? TextCell(target: isTarget)
            cell.identifier = column.identifier
            cell.controller = self
            cell.pid = r.pid
            cell.show(isTarget ? r.tgt : r.src)
            if isTarget { cell.textView.isEditable = r.editable }
            return cell
        }
    }

    func tableViewColumnDidResize(_ notification: Notification) {
        widthChanged(notification)
    }

    fileprivate func rowState(_ index: Int) -> (picked: Bool, active: Bool) {
        guard index < shown.count else { return (false, false) }
        return (picked.contains(shown[index].pid), shown[index].pid == activePid)
    }
}

final class SegmentRowView: NSTableRowView {
    weak var controller: SegmentsController?
    var index = 0

    override func drawBackground(in dirtyRect: NSRect) {
        let state = controller?.rowState(index) ?? (picked: false, active: false)
        let accent = NSColor.controlAccentColor
        let color: NSColor = state.picked ? accent.withAlphaComponent(0.22) : state.active ? accent.withAlphaComponent(0.08) : (index % 2 == 1 ? NSColor.alternatingContentBackgroundColors.last ?? .clear : .textBackgroundColor)
        color.setFill()
        bounds.fill()
        if state.picked || state.active {
            accent.setFill()
            NSRect(x: 0, y: 0, width: 3, height: bounds.height).fill()
        }
    }
}

final class NumberCell: NSTableCellView {
    private let number = NSTextField(labelWithString: "")
    private let part = NSTextField(labelWithString: "")
    private let icon = NSImageView()

    init() {
        super.init(frame: .zero)
        for l in [number, part] {
            l.font = .monospacedDigitSystemFont(ofSize: NSFont.smallSystemFontSize, weight: .regular)
            l.textColor = .secondaryLabelColor
        }
        let stack = NSStackView(views: [number, part, icon])
        stack.orientation = .vertical
        stack.alignment = .leading
        stack.spacing = 2
        stack.translatesAutoresizingMaskIntoConstraints = false
        addSubview(stack)
        NSLayoutConstraint.activate([
            stack.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 10),
            stack.topAnchor.constraint(equalTo: topAnchor, constant: TextCell.inset + 1),
        ])
    }

    required init?(coder: NSCoder) { fatalError() }

    func show(_ r: Row) {
        number.stringValue = "\(r.n)"
        part.stringValue = r.part
        part.isHidden = r.part.isEmpty
        icon.isHidden = !(r.done || r.edited)
        icon.image = NSImage(systemSymbolName: r.done ? "checkmark.circle.fill" : "pencil", accessibilityDescription: r.done ? "Done" : "Edited")
        icon.contentTintColor = r.done ? .systemGreen : .linkColor
    }
}

final class TextCell: NSTableCellView, NSTextViewDelegate {
    static let inset: CGFloat = 8
    weak var controller: SegmentsController?
    var pid: String?
    let isTarget: Bool
    let textView: RowTextView
    /// What the field holds, as the editor has it (before typing is answered).
    var known = ""
    private var updating = false

    init(target: Bool) {
        isTarget = target
        textView = RowTextView(frame: .zero)
        super.init(frame: .zero)
        textView.cell = self
        textView.isRichText = true
        textView.isEditable = target
        textView.isSelectable = true
        textView.allowsUndo = false
        textView.drawsBackground = false
        textView.isVerticallyResizable = false
        textView.isHorizontallyResizable = false
        textView.textContainer?.widthTracksTextView = true
        textView.textContainer?.lineFragmentPadding = 5
        textView.textContainerInset = .zero
        textView.isAutomaticQuoteSubstitutionEnabled = false
        textView.isAutomaticDashSubstitutionEnabled = false
        textView.isAutomaticTextReplacementEnabled = false
        textView.isAutomaticSpellingCorrectionEnabled = false
        textView.isContinuousSpellCheckingEnabled = false
        textView.isGrammarCheckingEnabled = false
        textView.usesFindBar = false
        textView.delegate = self
        textView.setAccessibilityLabel(target ? "Translation" : "Original")
        textView.translatesAutoresizingMaskIntoConstraints = false
        addSubview(textView)
        NSLayoutConstraint.activate([
            textView.leadingAnchor.constraint(equalTo: leadingAnchor, constant: Self.inset),
            textView.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -Self.inset),
            textView.topAnchor.constraint(equalTo: topAnchor, constant: Self.inset),
            textView.bottomAnchor.constraint(equalTo: bottomAnchor, constant: -Self.inset),
        ])
    }

    required init?(coder: NSCoder) { fatalError() }

    /// Shows spans; a focused field keeps its selection.
    func show(_ spans: [JSON]) {
        let sel = textView.selectedRange()
        updating = true
        textView.textStorage?.setAttributedString(Spans.attributed(spans))
        textView.typingAttributes = [.font: Spans.font, .foregroundColor: NSColor.textColor]
        updating = false
        known = textView.string
        if textView.window?.firstResponder === textView {
            let len = (textView.string as NSString).length
            let loc = min(sel.location, len)
            textView.setSelectedRange(NSRange(location: loc, length: min(sel.length, len - loc)))
        }
    }

    func textDidChange(_ notification: Notification) {
        guard isTarget, !updating else { return }
        controller?.typed(self)
    }

    func textView(_ textView: NSTextView, doCommandBy selector: Selector) -> Bool {
        isTarget ? controller?.command(selector, in: self) ?? false : false
    }

    func textView(_ view: NSTextView, menu: NSMenu, for event: NSEvent, at charIndex: Int) -> NSMenu? {
        guard let controller, let pid else { return nil }
        if controller.pickCount > 0 && controller.isPicked(pid) { return controller.rowsMenu() }
        controller.clearPicks()
        if isTarget {
            if textView.selectedRange().length == 0 {
                let p = textView.convert(event.locationInWindow, from: nil)
                let i = textView.characterIndexForInsertion(at: p)
                textView.setSelectedRange(NSRange(location: i, length: 0))
            }
            if textView.window?.firstResponder !== textView { textView.window?.makeFirstResponder(textView) }
            controller.sync(self)
            return controller.editor?.contextMenu()
        }
        let sel = (textView.string as NSString).substring(with: textView.selectedRange())
        return controller.sourceMenu(pid, selected: sel, in: textView, at: textView.convert(event.locationInWindow, from: nil))
    }
}

/// A row's text: picks rows on Shift- and ⌘-click, and sends the editing
/// actions to the editor (after giving it the row's selection).
final class RowTextView: NSTextView {
    weak var cell: TextCell?

    private var editor: EditorController? { cell?.controller?.editor }

    override func becomeFirstResponder() -> Bool {
        let ok = super.becomeFirstResponder()
        if ok, let cell, cell.isTarget { cell.controller?.focused(cell) }
        return ok
    }

    override func resignFirstResponder() -> Bool {
        let ok = super.resignFirstResponder()
        // Leaving the row: shown as the editor has it (formatting of typed text).
        if ok, let cell, cell.isTarget, let c = cell.controller, let pid = cell.pid {
            c.reshow(pid)
        }
        return ok
    }

    override func mouseDown(with event: NSEvent) {
        guard let cell, let c = cell.controller, let pid = cell.pid else { return super.mouseDown(with: event) }
        let shift = event.modifierFlags.contains(.shift), cmd = event.modifierFlags.contains(.command)
        if shift && (c.pickCount > 0 || pid != c.activePid) {
            c.pickRange(pid)
            return
        }
        if cmd && !shift {
            c.pickToggle(pid)
            return
        }
        c.clearPicks()
        super.mouseDown(with: event)
    }

    private func synced(_ f: (EditorController) -> Void) {
        guard let cell, cell.isTarget, let c = cell.controller, let editor = c.editor else { return }
        c.sync(cell)
        f(editor)
    }

    @objc func undo(_ sender: Any?) { synced { $0.undo(sender) } }
    @objc func redo(_ sender: Any?) { synced { $0.redo(sender) } }
    override func paste(_ sender: Any?) {
        guard cell?.isTarget == true else { return }
        synced { $0.paste(sender) }
    }
    override func cut(_ sender: Any?) {
        guard cell?.isTarget == true else { return }
        synced { $0.cut(sender) }
    }
    override func copy(_ sender: Any?) {
        guard cell?.isTarget == true else { return super.copy(sender) }
        synced { $0.copySelection(cut: false) }
    }
    @objc func toggleBold(_ sender: Any?) { synced { $0.toggleBold(sender) } }
    @objc func toggleItalic(_ sender: Any?) { synced { $0.toggleItalic(sender) } }
    @objc func toggleUnderline(_ sender: Any?) { synced { $0.toggleUnderline(sender) } }
    @objc func pasteLink(_ sender: Any?) { synced { $0.pasteLink(sender) } }
    @objc func revertParagraph(_ sender: Any?) { synced { $0.revertParagraph(sender) } }
    @objc func copyOriginal(_ sender: Any?) { synced { $0.copyOriginal(sender) } }

    @objc func markDone(_ sender: Any?) {
        guard let cell, let c = cell.controller, let pid = cell.pid else { return }
        if c.pickCount > 0 { c.doneActive() } else { c.done(pid, cell: cell) }
    }

    @objc func insertClaude(_ sender: Any?) {
        guard let cell, let c = cell.controller, let pid = cell.pid else { return }
        if c.pickCount > 0 { c.rowsAction("claude") } else { c.insertClaude(pid, cell: cell) }
    }

    override func validateMenuItem(_ item: NSMenuItem) -> Bool {
        guard let editor, cell?.isTarget == true else { return super.validateMenuItem(item) }
        switch item.action {
        case #selector(undo(_:)), #selector(redo(_:)), #selector(markDone(_:)), #selector(insertClaude(_:)),
             #selector(toggleBold(_:)), #selector(toggleItalic(_:)), #selector(toggleUnderline(_:)),
             #selector(pasteLink(_:)), #selector(revertParagraph(_:)), #selector(copyOriginal(_:)):
            // The editor's state follows the row only once synced; these act on the row's selection.
            let mapped = NSMenuItem(title: item.title, action: Self.editorAction(item.action!), keyEquivalent: "")
            let ok = editor.validateMenuItem(mapped)
            item.title = mapped.title
            switch item.action {
            case #selector(toggleBold(_:)), #selector(toggleItalic(_:)), #selector(toggleUnderline(_:)), #selector(pasteLink(_:)):
                return selectedRange().length > 0 || ok
            case #selector(markDone(_:)): return cell?.controller?.activePid.isEmpty == false
            case #selector(insertClaude(_:)): return editor.claude != nil
            case #selector(revertParagraph(_:)), #selector(copyOriginal(_:)): return true
            default: return ok
            }
        case #selector(cut(_:)), #selector(copy(_:)): return selectedRange().length > 0
        case #selector(paste(_:)): return isEditable
        default: return super.validateMenuItem(item)
        }
    }

    private static func editorAction(_ s: Selector) -> Selector {
        switch s {
        case #selector(undo(_:)): return #selector(EditorController.undo(_:))
        case #selector(redo(_:)): return #selector(EditorController.redo(_:))
        case #selector(markDone(_:)): return #selector(EditorController.markDone(_:))
        case #selector(insertClaude(_:)): return #selector(EditorController.insertClaude(_:))
        case #selector(toggleBold(_:)): return #selector(EditorController.toggleBold(_:))
        case #selector(toggleItalic(_:)): return #selector(EditorController.toggleItalic(_:))
        case #selector(toggleUnderline(_:)): return #selector(EditorController.toggleUnderline(_:))
        case #selector(pasteLink(_:)): return #selector(EditorController.pasteLink(_:))
        case #selector(revertParagraph(_:)): return #selector(EditorController.revertParagraph(_:))
        default: return #selector(EditorController.copyOriginal(_:))
        }
    }

    /// The caret is on the text's first (last) line: Up (Down) goes to the row before (after).
    var caretOnFirstLine: Bool { caretLine.map { $0.minY <= 1 } ?? true }
    var caretOnLastLine: Bool {
        guard let lm = layoutManager, let tc = textContainer, let line = caretLine else { return true }
        return line.maxY >= lm.usedRect(for: tc).maxY - 1
    }

    private var caretLine: CGRect? {
        guard let lm = layoutManager, let tc = textContainer else { return nil }
        lm.ensureLayout(for: tc)
        let len = (string as NSString).length
        if len == 0 { return nil }
        let loc = selectedRange().location
        if loc >= len {
            let extra = lm.extraLineFragmentRect
            if extra.height > 0 { return extra }
            return lm.lineFragmentRect(forGlyphAt: max(0, lm.numberOfGlyphs - 1), effectiveRange: nil)
        }
        return lm.lineFragmentRect(forGlyphAt: lm.glyphIndexForCharacter(at: loc), effectiveRange: nil)
    }
}

extension SegmentsController {
    /// A row's translation shown again as the editor has it.
    fileprivate func reshow(_ pid: String) {
        guard let i = shownAt[pid], let cell = table.view(atColumn: 2, row: i, makeIfNecessary: false) as? TextCell else { return }
        cell.show(shown[i].tgt)
    }
}

// ── The side panel ────────────────────────────────────────────────────────

struct SegmentsPanelView: View {
    @Bindable var panel: SegmentsPanel

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 12) {
                if panel.pickCount > 0 {
                    Text("\(panel.pickCount) segments picked").font(.headline)
                    Text("Shift-click picks a range of segments, ⌘-click one more or one less. Esc or a click in a segment puts them down.")
                        .font(.callout).foregroundStyle(.secondary)
                    if panel.attached {
                        Button { panel.act("claude") } label: { Label("Insert Claude’s Translation", systemImage: "text.insert") }
                            .disabled(panel.pickedClaude == 0)
                            .help("⌘1: in the \(panel.pickedClaude) of them Claude has translated")
                    } else {
                        Button { panel.act("attach") } label: { Label("Attach Claude’s Translation…", systemImage: "doc.badge.plus") }
                    }
                    Button { panel.act("done") } label: { Label("Mark as Done", systemImage: "checkmark.circle") }
                    Button { panel.act("undone") } label: { Label("Unmark as Done", systemImage: "circle") }
                        .disabled(panel.pickedDone == 0)
                    Button { panel.act("revert") } label: { Label("Revert to the Original", systemImage: "arrow.uturn.backward") }
                        .disabled(panel.pickedEdited == 0)
                    Button { panel.act("putDown") } label: { Label("Put Down", systemImage: "xmark") }
                } else {
                    Text(panel.segment).font(.headline)
                    Text("Claude").font(.subheadline.bold())
                    if let t = panel.claudeText {
                        Text(t).textSelection(.enabled).frame(maxWidth: .infinity, alignment: .leading)
                        Button { panel.act("insert") } label: { Label("Insert Claude’s Translation", systemImage: "text.insert") }
                            .help("⌘1")
                    } else {
                        Text(panel.attached ? "Claude’s translation has no segment for this row." : "No translation by Claude is attached to this document.")
                            .font(.callout).foregroundStyle(.secondary)
                    }
                    if !panel.attached {
                        Button { panel.act("attach") } label: { Label("Attach Claude’s Translation…", systemImage: "doc.badge.plus") }
                    }
                    Divider()
                    Button { panel.act("done") } label: {
                        Label(panel.done ? "Unmark as Done" : "Mark as Done", systemImage: "checkmark.circle")
                    }
                    .disabled(!panel.hasActive)
                    .help("⌘Return: and go to the next segment")
                    Button { panel.act("revert") } label: { Label("Revert to the Original", systemImage: "arrow.uturn.backward") }
                        .disabled(!panel.edited)
                }
            }
            .padding(16)
            .frame(maxWidth: .infinity, alignment: .leading)
        }
        .background(Color(nsColor: .windowBackgroundColor))
    }
}
