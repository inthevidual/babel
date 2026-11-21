// swift-tools-version:5.9
// Babel for macOS: the window in Swift (AppKit, SwiftUI for the dialogs),
// everything else in Rust (crates/babel-mac, a static library). Build with
// packaging/macos/build-app.sh, which builds the library first.

import PackageDescription
import Foundation

let root = URL(fileURLWithPath: #filePath).deletingLastPathComponent().deletingLastPathComponent().path

let package = Package(
    name: "Babel",
    platforms: [.macOS(.v14)],
    targets: [
        .systemLibrary(name: "CBabel", path: "Sources/CBabel"),
        .executableTarget(
            name: "Babel",
            dependencies: ["CBabel"],
            path: "Sources/Babel",
            linkerSettings: [
                .unsafeFlags(["-L\(root)/target/release"]),
                .linkedFramework("AppKit"),
                .linkedFramework("Security"),
                .linkedFramework("SystemConfiguration"),
                .linkedFramework("CoreFoundation"),
            ]
        ),
    ]
)
