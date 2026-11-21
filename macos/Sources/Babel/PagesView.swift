// A document's pages, one under the other: drawn by the Rust side on demand
// (only those on screen are kept), with the selection, the caret, marks
// beside edited and done paragraphs, and proofing underlines on top. The
// translation's view takes the keyboard, input methods included: AppKit's
// key bindings name the commands (moveLeft:, deleteBackward:,
// insertNewline:, …) and they become the editor's keys.

import AppKit

final class PagesView: NSView, NSTextInputClient, NSMenuItemValidation {
    let side: Core.Side
    weak var editor: EditorController?

    var zoom: CGFloat = 1 {
        didSet { if zoom != oldValue { relayout() } }
    }

    private var pages: [PageInfo] = []
    private var frames: [CGRect] = []
    private struct Drawn {
        var version: Int
        var scale: CGFloat
        var image: CGImage
    }
    private var images: [Int: Drawn] = [:]
    private var rendering: Set<String> = []
    /// Counts documents shown, so a page drawn for the one before is dropped.
    private var epoch = 0
    private static let renderQueue = DispatchQueue(label: "se.cptjanst.babel.render", qos: .userInitiated)

    private var caretOn = true
    private var blink: Timer?
    private var marked = ""
    private var dragging = false

    private let gap: CGFloat = 18
    private let margin: CGFloat = 24
    /// Beside the pages, when the window is narrow.
    private let edge: CGFloat = 10

    /// View points per point of the document (at 100 %, a point is a point, as in Pages).
    var px: CGFloat { zoom }

    init(side: Core.Side) {
        self.side = side
        super.init(frame: .zero)
        setAccessibilityRole(.textArea)
        setAccessibilityLabel(side == .target ? "Translation" : "Original")
    }

    required init?(coder: NSCoder) { fatalError() }

    override var isFlipped: Bool { true }
    override var isOpaque: Bool { false }
    override var acceptsFirstResponder: Bool { side == .target }
    override func acceptsFirstMouse(for event: NSEvent?) -> Bool { true }

    // ── Pages ────────────────────────────────────────────────────────────

    /// The pages to show; `fresh` for another document (nothing drawn before is kept).
    func setPages(_ list: [PageInfo], fresh: Bool = false) {
        pages = list
        images = fresh ? [:] : images.filter { $0.key < list.count }
        if fresh {
            rendering = []
            epoch += 1
        }
        relayout()
    }

    /// Some pages were drawn again (new generations).
    func update(_ list: [PageInfo]) {
        for p in list where p.index < pages.count {
            pages[p.index] = p
            if p.index < frames.count { setNeedsDisplay(frames[p.index].insetBy(dx: -20, dy: -4)) }
        }
        if list.contains(where: { $0.index >= pages.count }) { relayout() }
    }

    func relayout() {
        guard let clip = enclosingScrollView?.contentView else { return }
        let widest = pages.map { $0.size.width * px }.max() ?? 0
        let width = max(clip.bounds.width, widest + 2 * edge)
        var y = margin
        frames = pages.map { p in
            let w = p.size.width * px, h = p.size.height * px
            let r = CGRect(x: ((width - w) / 2).rounded(), y: y, width: w, height: h)
            y += h + gap
            return r
        }
        let height = max(y - gap + margin, clip.bounds.height)
        setFrameSize(NSSize(width: width, height: height))
        needsDisplay = true
    }

    /// The page and the point on it (in points) under a point of the view.
    private func locate(_ p: CGPoint) -> (Int, CGPoint)? {
        guard !frames.isEmpty else { return nil }
        // The nearest page, so a click between pages still lands somewhere.
        var best = 0
        var dist = CGFloat.infinity
        for (i, f) in frames.enumerated() {
            let d = p.y < f.minY ? f.minY - p.y : p.y > f.maxY ? p.y - f.maxY : 0
            if d < dist { dist = d; best = i }
            if d == 0 { break }
        }
        let f = frames[best]
        let x = min(max(p.x, f.minX), f.maxX)
        let y = min(max(p.y, f.minY), f.maxY - 1)
        return (best, CGPoint(x: (x - f.minX) / px, y: (y - f.minY) / px))
    }

    private func viewRect(_ r: PageRect) -> CGRect? {
        guard r.page < frames.count else { return nil }
        let f = frames[r.page]
        return CGRect(x: f.minX + r.rect.minX * px, y: f.minY + r.rect.minY * px, width: r.rect.width * px, height: r.rect.height * px)
    }

    func pageFrame(_ i: Int) -> CGRect? {
        i < frames.count ? frames[i] : nil
    }

    func caretRect() -> CGRect? {
        guard side == .target, let c = editor?.caret, c.page < frames.count else { return nil }
        let f = frames[c.page]
        return CGRect(x: f.minX + c.x * px - 1, y: f.minY + c.top * px, width: 2, height: max(c.height * px, 4))
    }

    // ── Drawing ──────────────────────────────────────────────────────────

    override func draw(_ dirty: NSRect) {
        guard let ctx = NSGraphicsContext.current?.cgContext, let editor else { return }
        let scale = (window?.backingScaleFactor ?? 2) * px
        let visible = visibleRect
        for (i, f) in frames.enumerated() where f.intersects(dirty) {
            // The sheet of paper.
            ctx.saveGState()
            ctx.setShadow(offset: CGSize(width: 0, height: -1), blur: 4, color: NSColor.black.withAlphaComponent(0.25).cgColor)
            ctx.setFillColor(NSColor.white.cgColor)
            ctx.fill(f)
            ctx.restoreGState()
            if let img = image(i, scale: scale) {
                ctx.saveGState()
                ctx.interpolationQuality = .high
                // A CGImage draws bottom up; the view is flipped.
                ctx.translateBy(x: f.minX, y: f.maxY)
                ctx.scaleBy(x: 1, y: -1)
                ctx.draw(img, in: CGRect(origin: .zero, size: f.size))
                ctx.restoreGState()
            }
        }
        drawOverlays(ctx, editor: editor, dirty: dirty)
        forget(around: visible)
    }

    private func drawOverlays(_ ctx: CGContext, editor: EditorController, dirty: NSRect) {
        let accent = NSColor.selectedTextBackgroundColor.withAlphaComponent(0.55)
        if side == .source {
            ctx.setFillColor(accent.cgColor)
            for r in editor.sourceSelection { if let v = viewRect(r) { ctx.fill(v) } }
            return
        }
        // Edited (blue) and done (green) paragraphs, beside them.
        if editor.showMarks {
            for m in editor.marks {
                guard var v = viewRect(m) else { continue }
                v.origin.x -= 9 * px
                v.size.width = max(2, 2.5 * px)
                let color = m.kind == "done" ? NSColor.systemGreen : NSColor.systemBlue
                ctx.setFillColor(color.withAlphaComponent(0.8).cgColor)
                ctx.addPath(CGPath(roundedRect: v, cornerWidth: v.width / 2, cornerHeight: v.width / 2, transform: nil))
                ctx.fillPath()
            }
        }
        ctx.setFillColor(accent.cgColor)
        for r in editor.selection {
            guard var v = viewRect(r) else { continue }
            v.size.width = max(2, v.width)
            ctx.fill(v)
        }
        // Proofing underlines.
        for u in editor.underlines {
            guard let v = viewRect(u) else { continue }
            let color: NSColor = u.kind == "spelling" ? .systemRed : u.kind == "style" ? .systemGreen : .systemBlue
            ctx.setStrokeColor(color.withAlphaComponent(0.9).cgColor)
            ctx.setLineWidth(max(1.2, 1.2 * px))
            ctx.setLineDash(phase: 0, lengths: u.kind == "spelling" ? [] : [2 * px, 1.5 * px])
            ctx.move(to: CGPoint(x: v.minX, y: v.minY + 0.75 * px))
            ctx.addLine(to: CGPoint(x: v.maxX, y: v.minY + 0.75 * px))
            ctx.strokePath()
        }
        ctx.setLineDash(phase: 0, lengths: [])
        // The caret, and what an input method is composing there.
        if let c = editor.caret, let r = caretRect(), editor.selection.isEmpty {
            if !marked.isEmpty {
                let font = NSFont.systemFont(ofSize: max(9, r.height * 0.75))
                let text = NSAttributedString(string: marked, attributes: [.font: font, .underlineStyle: NSUnderlineStyle.single.rawValue, .foregroundColor: NSColor.black])
                let size = text.size()
                let box = CGRect(x: r.minX, y: r.minY, width: size.width + 4, height: r.height)
                ctx.setFillColor(NSColor.white.withAlphaComponent(0.92).cgColor)
                ctx.fill(box)
                text.draw(at: CGPoint(x: box.minX + 2, y: box.minY + (box.height - size.height) / 2))
            } else if caretOn && window?.firstResponder === self && window?.isKeyWindow == true {
                ctx.setFillColor((c.editable ? NSColor.black : NSColor.gray).cgColor)
                ctx.fill(r)
            }
        }
    }

    /// The page's image at this scale, drawn now if it is new from an edit
    /// (no flicker), else in the background (an older one, or nothing, meanwhile).
    private func image(_ i: Int, scale: CGFloat) -> CGImage? {
        let want = pages[i].version
        let have = images[i]
        if let have, have.version == want, abs(have.scale - scale) < 0.01 { return have.image }
        if side == .target, let have, have.version != want, abs(have.scale - scale) < 0.01 {
            if let img = Core.render(side, page: i, scale: scale) {
                images[i] = Drawn(version: want, scale: scale, image: img)
                return img
            }
        }
        let key = "\(i)/\(want)/\(scale)"
        if !rendering.contains(key) {
            rendering.insert(key)
            let side = self.side, epoch = self.epoch
            Self.renderQueue.async { [weak self] in
                let img = Core.render(side, page: i, scale: scale)
                DispatchQueue.main.async {
                    guard let self, self.epoch == epoch else { return }
                    self.rendering.remove(key)
                    guard let img, i < self.pages.count, self.pages[i].version == want else { return }
                    self.images[i] = Drawn(version: want, scale: scale, image: img)
                    if i < self.frames.count { self.setNeedsDisplay(self.frames[i]) }
                }
            }
        }
        return have?.image
    }

    /// Only pages near the screen keep their images.
    private func forget(around visible: CGRect) {
        let near = visible.insetBy(dx: 0, dy: -visible.height * 1.5)
        for (i, _) in images where i >= frames.count || !frames[i].intersects(near) {
            images[i] = nil
        }
    }

    // ── The caret ────────────────────────────────────────────────────────

    /// The caret moved or the selection changed: drawn now, blinking again.
    func caretMoved() {
        caretOn = true
        blink?.invalidate()
        if side == .target {
            blink = Timer.scheduledTimer(withTimeInterval: 0.53, repeats: true) { [weak self] _ in
                guard let self, let r = self.caretRect() else { return }
                self.caretOn.toggle()
                self.setNeedsDisplay(r.insetBy(dx: -2, dy: -2))
            }
        }
        needsDisplay = true
        inputContext?.invalidateCharacterCoordinates()
    }

    override func becomeFirstResponder() -> Bool {
        needsDisplay = true
        return true
    }

    override func resignFirstResponder() -> Bool {
        needsDisplay = true
        return true
    }

    override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        relayout()
    }

    override func resetCursorRects() {
        addCursorRect(visibleRect, cursor: .iBeam)
    }

    // ── Mouse ────────────────────────────────────────────────────────────

    override func mouseDown(with event: NSEvent) {
        guard let editor else { return }
        // The keyboard is the translation's, whichever side is clicked.
        window?.makeFirstResponder(editor.target)
        if event.modifierFlags.contains(.control) {
            rightMouseDown(with: event)
            return
        }
        guard let (page, p) = locate(convert(event.locationInWindow, from: nil)) else { return }
        dragging = event.clickCount == 1
        editor.click(side, page: page, at: p, count: event.clickCount, extend: event.modifierFlags.contains(.shift))
    }

    override func mouseDragged(with event: NSEvent) {
        guard dragging, let editor else { return }
        let local = convert(event.locationInWindow, from: nil)
        autoscroll(with: event)
        guard let (page, p) = locate(local) else { return }
        editor.click(side, page: page, at: p, count: 1, extend: true, dragging: true)
    }

    override func mouseUp(with event: NSEvent) {
        dragging = false
    }

    override func rightMouseDown(with event: NSEvent) {
        guard let editor, editor.editing else { return }
        let local = convert(event.locationInWindow, from: nil)
        guard let (page, p) = locate(local) else { return }
        window?.makeFirstResponder(editor.target)
        let menu = side == .target ? editor.targetMenu(page: page, at: p) : editor.sourceMenu(page: page, at: p, in: self, viewPoint: local)
        NSMenu.popUpContextMenu(menu, with: event, for: self)
    }

    override func menu(for event: NSEvent) -> NSMenu? { nil }

    // ── Keyboard ─────────────────────────────────────────────────────────

    override func keyDown(with event: NSEvent) {
        guard side == .target, let editor, editor.editing else {
            super.keyDown(with: event)
            return
        }
        if event.keyCode == 53, marked.isEmpty {  // Esc
            editor.hidePopover()
            return
        }
        inputContext?.handleEvent(event)
    }

    /// AppKit's key bindings, as the editor's keys.
    override func doCommand(by selector: Selector) {
        guard let editor else { return }
        let shift = NSApp.currentEvent?.modifierFlags.contains(.shift) ?? false
        let keys: [String: (String, Bool)] = [
            "moveLeft:": ("left", false), "moveRight:": ("right", false),
            "moveUp:": ("up", false), "moveDown:": ("down", false),
            "moveBackward:": ("left", false), "moveForward:": ("right", false),
            "moveLeftAndModifySelection:": ("left", true), "moveRightAndModifySelection:": ("right", true),
            "moveUpAndModifySelection:": ("up", true), "moveDownAndModifySelection:": ("down", true),
            "moveBackwardAndModifySelection:": ("left", true), "moveForwardAndModifySelection:": ("right", true),
            "moveToBeginningOfLine:": ("home", false), "moveToLeftEndOfLine:": ("home", false), "moveToBeginningOfParagraph:": ("home", false),
            "moveToEndOfLine:": ("end", false), "moveToRightEndOfLine:": ("end", false), "moveToEndOfParagraph:": ("end", false),
            "moveToBeginningOfLineAndModifySelection:": ("home", true), "moveToLeftEndOfLineAndModifySelection:": ("home", true),
            "moveParagraphBackwardAndModifySelection:": ("home", true), "moveToBeginningOfParagraphAndModifySelection:": ("home", true),
            "moveToEndOfLineAndModifySelection:": ("end", true), "moveToRightEndOfLineAndModifySelection:": ("end", true),
            "moveParagraphForwardAndModifySelection:": ("end", true), "moveToEndOfParagraphAndModifySelection:": ("end", true),
            "deleteBackward:": ("backspace", false), "deleteBackwardByDecomposingPreviousCharacter:": ("backspace", false),
            "deleteForward:": ("delete", false),
            "insertLineBreak:": ("lineBreak", false), "insertNewlineIgnoringFieldEditor:": ("lineBreak", false),
        ]
        let name = NSStringFromSelector(selector)
        if let (key, extend) = keys[name] {
            editor.key(key, extend: extend)
        } else if name == "insertNewline:" {
            editor.key(shift ? "lineBreak" : "enter")
        } else if name == "insertTab:" {
            editor.type("\t")
        } else if name == "cancelOperation:" {
            editor.hidePopover()
        } else if name == "scrollPageUp:" || name == "pageUp:" {
            enclosingScrollView?.pageUp(nil)
        } else if name == "scrollPageDown:" || name == "pageDown:" {
            enclosingScrollView?.pageDown(nil)
        } else if name == "scrollToBeginningOfDocument:" || name == "moveToBeginningOfDocument:" {
            scroll(.zero)
        } else if name == "scrollToEndOfDocument:" || name == "moveToEndOfDocument:" {
            scroll(CGPoint(x: 0, y: bounds.height))
        }
    }

    // Edit menu items reach the editor from here.
    @objc func undo(_ sender: Any?) { editor?.undo(sender) }
    @objc func redo(_ sender: Any?) { editor?.redo(sender) }
    @objc func cut(_ sender: Any?) { editor?.cut(sender) }
    @objc func copy(_ sender: Any?) { editor?.copy(sender) }
    @objc func paste(_ sender: Any?) { editor?.paste(sender) }
    @objc override func selectAll(_ sender: Any?) { editor?.selectAll(sender) }

    func validateMenuItem(_ item: NSMenuItem) -> Bool {
        editor?.validateMenuItem(item) ?? false
    }

    // ── NSTextInputClient ────────────────────────────────────────────────

    func insertText(_ string: Any, replacementRange: NSRange) {
        let text = (string as? NSAttributedString)?.string ?? (string as? String) ?? ""
        marked = ""
        guard !text.isEmpty else {
            needsDisplay = true
            return
        }
        editor?.type(text)
    }

    func setMarkedText(_ string: Any, selectedRange: NSRange, replacementRange: NSRange) {
        marked = (string as? NSAttributedString)?.string ?? (string as? String) ?? ""
        if let r = caretRect() { setNeedsDisplay(r.insetBy(dx: -4, dy: -4).union(CGRect(x: r.minX, y: r.minY, width: 600, height: r.height))) } else { needsDisplay = true }
    }

    func unmarkText() {
        marked = ""
        needsDisplay = true
    }

    func selectedRange() -> NSRange { NSRange(location: 0, length: 0) }

    func markedRange() -> NSRange {
        marked.isEmpty ? NSRange(location: NSNotFound, length: 0) : NSRange(location: 0, length: (marked as NSString).length)
    }

    func hasMarkedText() -> Bool { !marked.isEmpty }

    func attributedSubstring(forProposedRange range: NSRange, actualRange: NSRangePointer?) -> NSAttributedString? { nil }

    func validAttributesForMarkedText() -> [NSAttributedString.Key] { [] }

    /// Where candidate windows open: at the caret.
    func firstRect(forCharacterRange range: NSRange, actualRange: NSRangePointer?) -> NSRect {
        guard let r = caretRect(), let window else { return .zero }
        return window.convertToScreen(convert(r, to: nil))
    }

    func characterIndex(for point: NSPoint) -> Int { 0 }
}
