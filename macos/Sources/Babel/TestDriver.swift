// UI test driver: with BABEL_TEST_SCRIPT set to a JSON file of steps, runs
// them through real AppKit events (mouse and keys go through the window and
// the key bindings, menu shortcuts through the menu bar), logs the state,
// then quits. macos/tests/ has the scripts; packaging/macos/test-app.sh runs them.
//
// Steps: {"open": FILE, "lang": CODE}, {"project": ID}, {"click": [side, page,
// x, y], "count": N, "mods": ["shift", "cmd"]}, {"drag": [side, page, x0, y0,
// x1, y1]}, {"type": TEXT}, {"key": NAME, "mods": [...]}, {"textView": BOOL},
// {"menu": SELECTOR}, {"wait": MS}, {"grab": FILE.png}, {"log": LABEL},
// {"expect": {KEY: VALUE}}, each with an optional "name". Points are in the
// page's points; side is "s" (the original) or "t" (the translation).

import AppKit

enum TestDriver {
    private static var steps: [JSON] = []
    private static var at = 0
    private static var failures = 0

    static func startIfAsked() {
        guard let path = ProcessInfo.processInfo.environment["BABEL_TEST_SCRIPT"],
              let data = FileManager.default.contents(atPath: path),
              let list = (try? JSONSerialization.jsonObject(with: data)) as? [JSON] else { return }
        steps = list
        next(after: 0.8)
    }

    private static func next(after delay: Double) {
        DispatchQueue.main.asyncAfter(deadline: .now() + delay) { run() }
    }

    private static var wc: MainWindowController? { MainWindowController.shared }
    private static var editor: EditorController? { wc?.editor }

    private static func run() {
        guard at < steps.count else {
            print("UITEST done, \(failures) failed")
            fflush(stdout)
            NSApp.terminate(nil)
            return
        }
        let s = steps[at]
        at += 1
        var delay = 0.15
        if s.has("wait") { delay = s.num("wait") / 1000 }
        if let set = s.obj("settings") {
            var all = Core.object("settings") ?? [:]
            all.merge(set) { $1 }
            Core.call("setSettings", all)
        }
        if s.has("open") {
            AppModel.shared.make(original: URL(fileURLWithPath: s.str("open")), translated: nil, lang: s.has("lang") ? s.str("lang") : "sv-SE")
        }
        if s.has("project") { AppModel.shared.selection = s.str("project").isEmpty ? AppModel.shared.projects.first?.id : s.str("project") }
        if let a = s["signIn"] as? [String], a.count == 3 { AppModel.shared.signIn(server: a[0], user: a[1], password: a[2]) }
        if s.has("signOut") { AppModel.shared.signOut() }
        if s.has("ask") { AppModel.shared.openDocument(URL(fileURLWithPath: s.str("ask"))) }
        if let c = s["click"] as? [Any] { click(c, count: max(1, s.int("count")), mods: mods(s)) }
        if let d = s["drag"] as? [Any] { drag(d) }
        if s.has("type") {
            for ch in s.str("type") { key(String(ch), code: 0, chars: String(ch), mods: []) }
        }
        if s.has("key") { namedKey(s.str("key"), mods: mods(s)) }
        if s.has("textView") { editor?.setTextView(s.bool("textView")) }
        if s.has("onlyOpen") || s.has("onlyIssues") {
            editor?.segments.setFilters(open: s.has("onlyOpen") ? s.bool("onlyOpen") : nil, issues: s.has("onlyIssues") ? s.bool("onlyIssues") : nil)
        }
        if s.has("menu") { NSApp.sendAction(NSSelectorFromString(s.str("menu")), to: nil, from: nil) }
        if s.has("grab") { grab(s.str("grab")) }
        if s.has("export") { AppModel.report(Core.object("export", ["id": AppModel.shared.selection ?? "", "path": s.str("export")])) }
        if s.has("log") { print("UITEST", s.str("log"), stateJSON()) }
        if let want = s.obj("expect") { expect(want, name: s.str("name")) }
        fflush(stdout)
        next(after: delay)
    }

    private static func mods(_ s: JSON) -> NSEvent.ModifierFlags {
        var m: NSEvent.ModifierFlags = []
        for name in s["mods"] as? [String] ?? [] {
            switch name {
            case "shift": m.insert(.shift)
            case "cmd": m.insert(.command)
            case "alt": m.insert(.option)
            case "ctrl": m.insert(.control)
            default: break
            }
        }
        return m
    }

    // ── Mouse ────────────────────────────────────────────────────────────

    private static func windowPoint(_ a: [Any], from i: Int = 1) -> (PagesView, NSPoint)? {
        guard let editor, a.count >= i + 3 else { return nil }
        let view = (a[0] as? String) == "s" ? editor.source : editor.target
        let page = (a[i] as? NSNumber)?.intValue ?? 0
        let x = CGFloat((a[i + 1] as? NSNumber)?.doubleValue ?? 0), y = CGFloat((a[i + 2] as? NSNumber)?.doubleValue ?? 0)
        guard let p = view.viewPoint(page: page, x: x, y: y) else { return nil }
        view.scrollToVisible(NSRect(x: p.x - 40, y: p.y - 80, width: 80, height: 160))
        return (view, view.convert(p, to: nil))
    }

    private static func mouse(_ type: NSEvent.EventType, _ p: NSPoint, count: Int, mods: NSEvent.ModifierFlags) {
        guard let w = wc?.window,
              let e = NSEvent.mouseEvent(with: type, location: p, modifierFlags: mods, timestamp: ProcessInfo.processInfo.systemUptime,
                                         windowNumber: w.windowNumber, context: nil, eventNumber: 0, clickCount: count, pressure: 1) else { return }
        w.sendEvent(e)
    }

    private static func click(_ a: [Any], count: Int, mods: NSEvent.ModifierFlags) {
        guard let (_, p) = windowPoint(a) else { return }
        for n in 1...count {
            mouse(.leftMouseDown, p, count: n, mods: mods)
            mouse(.leftMouseUp, p, count: n, mods: mods)
        }
    }

    private static func drag(_ a: [Any]) {
        guard a.count >= 6, let (_, p0) = windowPoint(Array(a[0..<4])), let (_, p1) = windowPoint([a[0], a[1], a[4], a[5]]) else { return }
        mouse(.leftMouseDown, p0, count: 1, mods: [])
        for i in 1...10 {
            let t = CGFloat(i) / 10
            mouse(.leftMouseDragged, NSPoint(x: p0.x + (p1.x - p0.x) * t, y: p0.y + (p1.y - p0.y) * t), count: 1, mods: [])
        }
        mouse(.leftMouseUp, p1, count: 1, mods: [])
    }

    // ── Keys ─────────────────────────────────────────────────────────────

    private static func key(_ text: String, code: UInt16, chars: String, mods: NSEvent.ModifierFlags) {
        guard let w = wc?.window else { return }
        for type in [NSEvent.EventType.keyDown, .keyUp] {
            guard let e = NSEvent.keyEvent(with: type, location: .zero, modifierFlags: mods, timestamp: ProcessInfo.processInfo.systemUptime,
                                           windowNumber: w.windowNumber, context: nil, characters: chars,
                                           charactersIgnoringModifiers: text, isARepeat: false, keyCode: code) else { continue }
            // Menu shortcuts as AppKit gives them to an active app (the tests may run in the background).
            if type == .keyDown, mods.contains(.command), NSApp.mainMenu?.performKeyEquivalent(with: e) == true { continue }
            NSApp.sendEvent(e)
        }
    }

    private static func namedKey(_ name: String, mods: NSEvent.ModifierFlags) {
        func fn(_ k: Int) -> String { String(Character(UnicodeScalar(k)!)) }
        let table: [String: (UInt16, String)] = [
            "left": (123, fn(NSLeftArrowFunctionKey)), "right": (124, fn(NSRightArrowFunctionKey)),
            "down": (125, fn(NSDownArrowFunctionKey)), "up": (126, fn(NSUpArrowFunctionKey)),
            "return": (36, "\r"), "backspace": (51, "\u{7f}"), "delete": (117, fn(NSDeleteFunctionKey)),
            "tab": (48, "\t"), "escape": (53, "\u{1b}"),
        ]
        if let (code, chars) = table[name] {
            key(chars, code: code, chars: chars, mods: mods.union(code >= 123 ? [.numericPad, .function] : []))
        } else {
            key(name, code: 0, chars: name, mods: mods)
        }
    }

    // ── State ────────────────────────────────────────────────────────────

    private static func state() -> JSON {
        guard let e = editor else { return [:] }
        var st: JSON = [
            "editing": e.editing,
            "project": e.projectName,
            "caret": e.caret.map { ["page": $0.page, "x": Int($0.x)] } ?? NSNull(),
            "para": e.paragraph?.str("pid") ?? NSNull(),
            "done": e.paragraphDone,
            "selected": Core.string("selectedText"),
            "sourceSelected": Core.string("sourceSelectedText"),
            "chars": e.stats?.int("tgtChars") ?? 0,
            "doneCount": e.stats?.int("done") ?? 0,
            "bold": e.styles.b,
            "textView": e.textView,
            "textZoom": Double(Spans.zoom),
            "zoom": Double(e.zoom),
            "unsaved": e.unsaved,
            "projects": AppModel.shared.projects.count,
            "signedIn": AppModel.shared.signedIn,
            "syncStatus": AppModel.shared.syncStatus,
            "frame": NSStringFromRect(wc?.window?.frame ?? .zero),
            "firstResponder": String(describing: type(of: wc?.window?.firstResponder ?? NSObject())),
            "issues": e.issueCount,
        ]
        st.merge(e.segments.filterState) { $1 }
        return st
    }

    private static func stateJSON() -> String {
        let data = (try? JSONSerialization.data(withJSONObject: state(), options: [.sortedKeys])) ?? Data()
        return String(decoding: data, as: UTF8.self)
    }

    private static func expect(_ want: JSON, name: String) {
        let st = state()
        for (k, v) in want {
            let ok: Bool
            switch k {
            case "selectedStarts": ok = (st["selected"] as? String ?? "").hasPrefix(v as? String ?? "\u{0}")
            case "sourceStarts": ok = (st["sourceSelected"] as? String ?? "").hasPrefix(v as? String ?? "\u{0}")
            case "selectedSome": ok = !(st["selected"] as? String ?? "").isEmpty == (v as? Bool ?? false)
            case "charsBelow": ok = (st["chars"] as? Int ?? 0) < (v as? Int ?? 0)
            case "rowsFiltered": ok = ((st["rows"] as? Int ?? 0) < (st["rowsAll"] as? Int ?? 0)) == (v as? Bool ?? false)
            case "allWithIssues": ok = ((st["rows"] as? Int ?? 0) > 0 && st["rows"] as? Int == st["rowsWithIssues"] as? Int) == (v as? Bool ?? false)
            case "charsAbove": ok = (st["chars"] as? Int ?? 0) > (v as? Int ?? 0)
            default: ok = (st[k] as? NSObject)?.isEqual(v) ?? (v is NSNull)
            }
            if !ok { failures += 1 }
            print(ok ? "UITEST ok  " : "UITEST FAIL", name, k, "\(v)", ok ? "" : stateJSON())
        }
    }

    /// The window as on screen (screencapture), or as its views draw themselves.
    private static func grab(_ path: String) {
        guard let w = wc?.window else { return }
        let p = Process()
        p.executableURL = URL(fileURLWithPath: "/usr/sbin/screencapture")
        p.arguments = ["-x", "-o", "-l", String(w.windowNumber), path]
        try? FileManager.default.removeItem(atPath: path)
        if (try? p.run()) != nil {
            p.waitUntilExit()
            if FileManager.default.fileExists(atPath: path) { return }
        }
        guard let view = w.contentView?.superview ?? w.contentView,
              let rep = view.bitmapImageRepForCachingDisplay(in: view.bounds) else { return }
        view.cacheDisplay(in: view.bounds, to: rep)
        try? rep.representation(using: .png, properties: [:])?.write(to: URL(fileURLWithPath: path))
    }
}

extension PagesView {
    /// A point on a page (in points) in the view.
    func viewPoint(page: Int, x: CGFloat, y: CGFloat) -> NSPoint? {
        guard let f = pageFrame(page) else { return nil }
        return NSPoint(x: f.minX + x * px, y: f.minY + y * px)
    }
}
