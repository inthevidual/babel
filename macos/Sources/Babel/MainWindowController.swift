// The window: the translations in the sidebar, the editor beside them, and
// the toolbar. Editing actions that reach the window (from the menu bar or
// the toolbar, wherever the keyboard is) go on to the editor.

import AppKit
import SwiftUI

final class MainWindowController: NSWindowController, NSWindowDelegate {
    static weak var shared: MainWindowController?

    let editor = EditorController()
    private let split = NSSplitViewController()
    private var toolbarItems: [NSToolbarItem.Identifier: NSToolbarItem] = [:]

    init() {
        let window = NSWindow(
            contentRect: NSRect(x: 0, y: 0, width: 1500, height: 950),
            styleMask: [.titled, .closable, .miniaturizable, .resizable, .fullSizeContentView],
            backing: .buffered, defer: false)
        window.minSize = NSSize(width: 860, height: 560)
        window.title = "Babel"
        window.titlebarSeparatorStyle = .automatic
        window.toolbarStyle = .unified
        super.init(window: window)
        Self.shared = self
        window.delegate = self

        let sidebar = NSHostingController(rootView: ProjectsView(model: AppModel.shared))
        // The window's size is the user's, not SwiftUI's.
        sidebar.sizingOptions = []
        let sideItem = NSSplitViewItem(sidebarWithViewController: sidebar)
        sideItem.minimumThickness = 220
        sideItem.maximumThickness = 420
        sideItem.canCollapse = true
        split.addSplitViewItem(sideItem)
        let editorItem = NSSplitViewItem(viewController: editor)
        editorItem.minimumThickness = 600
        split.addSplitViewItem(editorItem)
        split.splitView.autosaveName = "BabelSplit"
        window.contentViewController = split
        // The content view controller sized the window to its views: the saved frame, or a large one.
        let screen = (NSScreen.main ?? NSScreen.screens.first)?.visibleFrame ?? NSRect(x: 0, y: 0, width: 1500, height: 950)
        window.setFrame(NSRect(x: 0, y: 0, width: min(1500, screen.width - 40), height: min(950, screen.height - 40)), display: false)
        window.center()
        window.setFrameUsingName("BabelMain")
        window.setFrameAutosaveName("BabelMain")

        let toolbar = NSToolbar(identifier: "BabelToolbar")
        toolbar.delegate = self
        toolbar.displayMode = .iconOnly
        toolbar.allowsUserCustomization = false
        window.toolbar = toolbar

        AppModel.shared.onSelect = { [weak self] id in self?.open(id) }
        editor.onChange = { [weak self] in self?.refresh() }
        refresh()
    }

    required init?(coder: NSCoder) { fatalError() }

    private func open(_ id: String?) {
        if let id { editor.load(id) } else { editor.close() }
        refresh()
    }

    /// The title, and the toolbar as the editor's state has it.
    func refresh() {
        guard let window else { return }
        if editor.editing {
            window.title = editor.projectName.isEmpty ? "Translation" : editor.projectName
            window.subtitle = "→ " + AppModel.shared.languageName(editor.targetLang)
        } else {
            window.title = "Babel"
            window.subtitle = AppModel.shared.projects.isEmpty ? "" : "Pick a translation, or open a document"
        }
        let e = editor
        if let g = toolbarItems[.format] as? NSToolbarItemGroup {
            g.isEnabled = e.editing
            g.setSelected(e.styles.b, at: 0)
            g.setSelected(e.styles.i, at: 1)
            g.setSelected(e.styles.u, at: 2)
            for (i, sub) in g.subitems.enumerated() {
                sub.isEnabled = e.editing && (e.hasSelection || [e.styles.b, e.styles.i, e.styles.u][i])
            }
        }
        toolbarItems[.link]?.isEnabled = e.editing && e.hasSelection
        if let done = toolbarItems[.done] {
            done.isEnabled = e.editing && e.paragraph != nil
            done.image = NSImage(systemSymbolName: e.paragraphDone ? "checkmark.circle.fill" : "checkmark.circle", accessibilityDescription: "Mark as Done")
            done.toolTip = e.paragraphDone ? "Unmark as Done (⌘↩)" : "Mark as Done (⌘↩)"
        }
        if let v = toolbarItems[.view] as? NSToolbarItemGroup {
            v.isEnabled = e.editing
            v.selectedIndex = e.textView ? 1 : 0
        }
        toolbarItems[.zoom]?.isEnabled = e.editing
        toolbarItems[.export]?.isEnabled = e.editing
        toolbarItems[.versions]?.isEnabled = e.editing
    }

    func windowWillClose(_ notification: Notification) {
        editor.close()
    }

    // The editor answers what reaches the window, so the menu bar and the
    // toolbar work wherever the keyboard is.
    override func supplementalTarget(forAction action: Selector, sender: Any?) -> Any? {
        if editor.responds(to: action) { return editor }
        return super.supplementalTarget(forAction: action, sender: sender)
    }

    // ── Toolbar actions ──────────────────────────────────────────────────

    @objc func formatClicked(_ sender: NSToolbarItemGroup) {
        switch sender.selectedIndex {
        case 0: editor.toggleBold(sender)
        case 1: editor.toggleItalic(sender)
        default: editor.toggleUnderline(sender)
        }
        refresh()
    }

    @objc func viewClicked(_ sender: NSToolbarItemGroup) {
        editor.setTextView(sender.selectedIndex == 1)
        refresh()
    }

    @objc func zoomClicked(_ sender: NSToolbarItemGroup) {
        if sender.selectedIndex == 0 { editor.zoomOut(sender) } else { editor.zoomIn(sender) }
    }
}

extension NSToolbarItem.Identifier {
    static let open = Self("open")
    static let format = Self("format")
    static let link = Self("link")
    static let done = Self("done")
    static let view = Self("view")
    static let zoom = Self("zoom")
    static let export = Self("export")
    static let versions = Self("versions")
}

extension MainWindowController: NSToolbarDelegate {
    func toolbarDefaultItemIdentifiers(_ toolbar: NSToolbar) -> [NSToolbarItem.Identifier] {
        [.toggleSidebar, .flexibleSpace, .open, .sidebarTrackingSeparator, .format, .link, .done, .flexibleSpace, .zoom, .view, .versions, .export]
    }

    func toolbarAllowedItemIdentifiers(_ toolbar: NSToolbar) -> [NSToolbarItem.Identifier] {
        toolbarDefaultItemIdentifiers(toolbar)
    }

    func toolbar(_ toolbar: NSToolbar, itemForItemIdentifier id: NSToolbarItem.Identifier, willBeInsertedIntoToolbar flag: Bool) -> NSToolbarItem? {
        let item: NSToolbarItem
        func button(_ symbol: String, _ label: String, _ action: Selector, tip: String? = nil) -> NSToolbarItem {
            let i = NSToolbarItem(itemIdentifier: id)
            i.image = NSImage(systemSymbolName: symbol, accessibilityDescription: label)
            i.label = label
            i.toolTip = tip ?? label
            i.action = action
            i.isBordered = true
            // The editor's items follow its state (refresh), not the responder chain.
            i.autovalidates = id == .open
            return i
        }
        switch id {
        case .open:
            item = button("plus", "Open Document", #selector(AppDelegate.openDocument(_:)), tip: "Open a Word document to translate (⌘O)")
        case .format:
            let g = NSToolbarItemGroup(
                itemIdentifier: id,
                images: ["bold", "italic", "underline"].compactMap { NSImage(systemSymbolName: $0, accessibilityDescription: $0) },
                selectionMode: .selectAny, labels: ["Bold", "Italic", "Underline"],
                target: self, action: #selector(formatClicked(_:)))
            g.label = "Format"
            g.subitems[0].toolTip = "Bold (⌘B)"
            g.subitems[1].toolTip = "Italic (⌘I)"
            g.subitems[2].toolTip = "Underline (⌘U)"
            item = g
        case .link:
            item = button("link", "Paste Link", #selector(EditorController.pasteLink(_:)), tip: "Make the selection a link to the copied link (⌘K)")
        case .done:
            item = button("checkmark.circle", "Mark as Done", #selector(EditorController.markDone(_:)), tip: "Mark as Done (⌘↩)")
        case .view:
            let g = NSToolbarItemGroup(
                itemIdentifier: id,
                images: ["doc.richtext", "list.bullet.rectangle"].compactMap { NSImage(systemSymbolName: $0, accessibilityDescription: $0) },
                selectionMode: .selectOne, labels: ["Pages", "Text"],
                target: self, action: #selector(viewClicked(_:)))
            g.label = "View"
            g.subitems[0].toolTip = "The pages as they will print (⌘T)"
            g.subitems[1].toolTip = "The segments side by side, as text (⌘T)"
            item = g
        case .zoom:
            let g = NSToolbarItemGroup(
                itemIdentifier: id,
                images: ["minus.magnifyingglass", "plus.magnifyingglass"].compactMap { NSImage(systemSymbolName: $0, accessibilityDescription: $0) },
                selectionMode: .momentary, labels: ["Zoom Out", "Zoom In"],
                target: self, action: #selector(zoomClicked(_:)))
            g.label = "Zoom"
            item = g
        case .versions:
            item = button("clock.arrow.circlepath", "Versions", #selector(EditorController.showVersions(_:)), tip: "Versions of this translation")
        case .export:
            item = button("square.and.arrow.up", "Export", #selector(EditorController.exportDocument(_:)), tip: "Export the translated .docx (⌘E)")
        default:
            return nil
        }
        if let g = item as? NSToolbarItemGroup { g.autovalidates = false }
        toolbarItems[id] = item
        return item
    }
}

// ── Sheets and alerts ─────────────────────────────────────────────────────

enum Sheets {
    /// Shows a SwiftUI view as a sheet on the window; it gets a way to close itself.
    static func present<V: View>(_ make: (@escaping () -> Void) -> V) {
        guard let window = MainWindowController.shared?.window else { return }
        var controller: NSViewController?
        let close = {
            if let c = controller { c.presentingViewController?.dismiss(c) }
            controller = nil
        }
        let host = NSHostingController(rootView: make(close))
        host.sizingOptions = [.preferredContentSize]
        controller = host
        window.contentViewController?.presentAsSheet(host)
    }

    static func alert(_ alert: NSAlert, _ done: @escaping (NSApplication.ModalResponse) -> Void) {
        if let w = MainWindowController.shared?.window { alert.beginSheetModal(for: w, completionHandler: done) } else { done(alert.runModal()) }
    }

    static func language(title: String, subtitle: String, current: String, ok: String, _ chosen: @escaping (String) -> Void) {
        present { close in
            LanguagePicker(title: title, subtitle: subtitle, languages: AppModel.shared.languages, current: current, ok: ok, close: close, chosen: chosen)
        }
    }

    static func pair() {
        present { close in PairView(close: close) }
    }

    static func account() {
        present { close in AccountView(model: AppModel.shared, close: close) }
    }

    static func versions(_ id: String, restored: @escaping () -> Void) {
        present { close in VersionsView(projectId: id, close: close, restored: restored) }
    }
}

// ── Messages ──────────────────────────────────────────────────────────────

/// A short message at the foot of the window that goes away by itself.
enum Toast {
    private static var current: NSView?
    private static var hide: DispatchWorkItem?

    static func show(_ text: String, error: Bool) {
        guard !text.isEmpty, let content = MainWindowController.shared?.window?.contentView else {
            if error { NSLog("Babel: %@", text) }
            return
        }
        current?.removeFromSuperview()
        hide?.cancel()

        let box = NSVisualEffectView()
        box.material = .hudWindow
        box.blendingMode = .withinWindow
        box.state = .active
        box.wantsLayer = true
        box.layer?.cornerRadius = 10
        box.translatesAutoresizingMaskIntoConstraints = false

        let icon = NSImageView(image: NSImage(systemSymbolName: error ? "exclamationmark.triangle.fill" : "info.circle", accessibilityDescription: nil)!)
        icon.contentTintColor = error ? .systemRed : .secondaryLabelColor
        let label = NSTextField(wrappingLabelWithString: text)
        label.font = .systemFont(ofSize: NSFont.systemFontSize)
        label.maximumNumberOfLines = 4
        label.preferredMaxLayoutWidth = 520
        let row = NSStackView(views: [icon, label])
        row.spacing = 8
        row.alignment = .firstBaseline
        row.translatesAutoresizingMaskIntoConstraints = false
        box.addSubview(row)
        content.addSubview(box)
        NSLayoutConstraint.activate([
            row.leadingAnchor.constraint(equalTo: box.leadingAnchor, constant: 14),
            row.trailingAnchor.constraint(equalTo: box.trailingAnchor, constant: -14),
            row.topAnchor.constraint(equalTo: box.topAnchor, constant: 10),
            row.bottomAnchor.constraint(equalTo: box.bottomAnchor, constant: -10),
            box.centerXAnchor.constraint(equalTo: content.centerXAnchor),
            box.bottomAnchor.constraint(equalTo: content.bottomAnchor, constant: -40),
            box.widthAnchor.constraint(lessThanOrEqualToConstant: 580),
        ])
        current = box
        NSAccessibility.post(element: label, notification: .announcementRequested, userInfo: [.announcement: text])
        let work = DispatchWorkItem {
            NSAnimationContext.runAnimationGroup({ $0.duration = 0.3; box.animator().alphaValue = 0 }, completionHandler: {
                box.removeFromSuperview()
                if current === box { current = nil }
            })
        }
        hide = work
        DispatchQueue.main.asyncAfter(deadline: .now() + (error || text.count > 80 ? 6 : 3), execute: work)
    }
}
