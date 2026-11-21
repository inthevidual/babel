// Babel's Rust side (crates/babel-mac): calls answered with JSON, pages
// drawn on demand, and events (proofing results, saves from elsewhere,
// sync) delivered on the main thread.

import AppKit
import CBabel

typealias JSON = [String: Any]

enum Core {
    enum Side: UInt8 {
        case target = 0
        case source = 1
    }

    /// Opens Babel's data (BABEL_DATA_DIR names another folder, for tests).
    static func start() throws {
        var err: UnsafeMutablePointer<CChar>?
        let dir = ProcessInfo.processInfo.environment["BABEL_DATA_DIR"]
        if !babel_start(dir, &err) {
            let text = err.map { String(cString: $0) } ?? "Babel could not start."
            if let err { babel_string_free(err) }
            throw NSError(domain: "Babel", code: 1, userInfo: [NSLocalizedDescriptionKey: text])
        }
    }

    /// Starts events; the Rust side starts syncing if signed in.
    static func connect() {
        babel_connect { ptr in
            guard let ptr else { return }
            let text = String(cString: ptr)
            DispatchQueue.main.async {
                guard let data = text.data(using: .utf8),
                      let event = (try? JSONSerialization.jsonObject(with: data)) as? JSON else { return }
                NotificationCenter.default.post(name: .babelEvent, object: nil, userInfo: event)
            }
        }
    }

    /// Calls `method`; the answer as JSON values (null is nil).
    @discardableResult
    static func call(_ method: String, _ args: JSON = [:]) -> Any? {
        let argData = (try? JSONSerialization.data(withJSONObject: args)) ?? Data("{}".utf8)
        let argText = String(decoding: argData, as: UTF8.self)
        guard let out = babel_call(method, argText) else { return nil }
        defer { babel_string_free(out) }
        let text = String(cString: out)
        guard let value = try? JSONSerialization.jsonObject(with: Data(text.utf8), options: [.fragmentsAllowed]) else { return nil }
        return value is NSNull ? nil : value
    }

    static func object(_ method: String, _ args: JSON = [:]) -> JSON? {
        call(method, args) as? JSON
    }

    static func list(_ method: String, _ args: JSON = [:]) -> [JSON] {
        call(method, args) as? [JSON] ?? []
    }

    static func string(_ method: String, _ args: JSON = [:]) -> String {
        call(method, args) as? String ?? ""
    }

    /// A page drawn at `scale` pixels per point.
    static func render(_ side: Side, page: Int, scale: CGFloat) -> CGImage? {
        var img = BabelImage()
        guard babel_render(side.rawValue, UInt32(page), Float(scale), &img), let data = img.data else { return nil }
        let w = Int(img.width), h = Int(img.height)
        // The pixels are copied into the image; Rust's buffer goes back now.
        let bytes = Data(bytes: data, count: img.len)
        babel_image_free(&img)
        guard let provider = CGDataProvider(data: bytes as CFData) else { return nil }
        return CGImage(
            width: w, height: h, bitsPerComponent: 8, bitsPerPixel: 32, bytesPerRow: w * 4,
            space: CGColorSpace(name: CGColorSpace.sRGB)!,
            bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.premultipliedLast.rawValue),
            provider: provider, decode: nil, shouldInterpolate: true, intent: .defaultIntent)
    }
}

extension Notification.Name {
    /// userInfo: the event's JSON ({"event": "issues" | "external" | "projects" | "sync" | "message", …}).
    static let babelEvent = Notification.Name("BabelEvent")
}

// ── Reading JSON ──────────────────────────────────────────────────────────

extension Dictionary where Key == String, Value == Any {
    func str(_ k: String) -> String { self[k] as? String ?? "" }
    func num(_ k: String) -> Double { (self[k] as? NSNumber)?.doubleValue ?? 0 }
    func int(_ k: String) -> Int { (self[k] as? NSNumber)?.intValue ?? 0 }
    func bool(_ k: String) -> Bool { (self[k] as? NSNumber)?.boolValue ?? false }
    func obj(_ k: String) -> JSON? { self[k] as? JSON }
    func arr(_ k: String) -> [JSON] { self[k] as? [JSON] ?? [] }
    func has(_ k: String) -> Bool { self[k] != nil }
}

/// A rectangle on a page, in points: selections, marks, underlines.
struct PageRect {
    var page: Int
    var rect: CGRect
    var kind: String

    init(_ j: JSON) {
        page = j.int("page")
        rect = CGRect(x: j.num("x"), y: j.num("y"), width: j.num("w"), height: j.num("h"))
        kind = j.str("kind")
    }

    static func list(_ a: [JSON]) -> [PageRect] { a.map(PageRect.init) }
}

/// The caret on a page of the translation, in points.
struct Caret: Equatable {
    var page: Int
    var x: CGFloat
    var top: CGFloat
    var height: CGFloat
    var editable: Bool

    init?(_ j: JSON?) {
        guard let j else { return nil }
        page = j.int("page")
        x = j.num("x")
        top = j.num("top")
        height = j.num("height")
        editable = j.bool("editable")
    }
}

/// A page as the editor lays it out: its size in points and drawing generation.
struct PageInfo {
    var index: Int
    var size: CGSize
    var version: Int

    init(_ j: JSON) {
        index = j.int("index")
        size = CGSize(width: j.num("w"), height: j.num("h"))
        version = j.int("v")
    }
}

/// The issue under the caret: what it is, and what to put instead.
struct Issue {
    var pid: String
    var n: Int
    var kind: String
    var message: String
    var short: String
    var replacements: [String]

    init?(_ j: JSON?) {
        guard let j else { return nil }
        pid = j.str("pid")
        n = j.int("n")
        kind = j.str("kind")
        message = j.str("message")
        short = j.str("short")
        replacements = j["replacements"] as? [String] ?? []
    }
}

enum Fmt {
    static func number(_ x: Double) -> String {
        let f = NumberFormatter()
        f.numberStyle = .decimal
        f.maximumFractionDigits = 0
        return f.string(from: NSNumber(value: x)) ?? String(Int(x))
    }

    static func percentDone(_ stats: JSON?) -> Int {
        guard let s = stats, s.num("total") > 0 else { return 0 }
        return Int((100 * s.num("done") / s.num("total")).rounded(.down))
    }

    static func ago(_ ms: Double) -> String {
        guard ms > 0 else { return "" }
        let date = Date(timeIntervalSince1970: ms / 1000)
        let s = Date().timeIntervalSince(date)
        if s < 60 { return "just now" }
        let f = RelativeDateTimeFormatter()
        f.unitsStyle = .short
        if s < 86400 { return f.localizedString(for: date, relativeTo: Date()) }
        return DateFormatter.localizedString(from: date, dateStyle: .short, timeStyle: .none)
    }

    static func when(_ ms: Double) -> String {
        DateFormatter.localizedString(from: Date(timeIntervalSince1970: ms / 1000), dateStyle: .medium, timeStyle: .short)
    }
}
