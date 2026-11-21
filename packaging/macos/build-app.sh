#!/usr/bin/env bash
# Builds Babel for macOS, the native app (macos/: Swift and AppKit, with
# Babel's Rust side as a static library, crates/babel-mac), as Babel.app.
# Needs Xcode's command line tools and Rust (rustup); no Qt, no Xcode.
#
#   packaging/macos/build-app.sh            # target/macos/Babel.app
#   packaging/macos/build-app.sh --install  # and into /Applications (or ~/Applications)
#
# Babel keeps its translations in ~/Library/Application Support/Babel.

set -euo pipefail

INSTALL=0
for a in "$@"; do
    case "$a" in
        --install) INSTALL=1 ;;
        -h|--help) sed -n '2,/^$/p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
        *) echo "Unknown option: $a (see --help)" >&2; exit 2 ;;
    esac
done

say() { printf '\n\033[1m== %s\033[0m\n' "$*"; }
die() { printf '\n\033[1;31m%s\033[0m\n' "$*" >&2; exit 1; }

[ "$(uname -s)" = Darwin ] || die "This script is for macOS."
REPO=$(cd "$(dirname "$0")/../.." && pwd)
OUT="$REPO/target/macos"
APP="$OUT/Babel.app"
export MACOSX_DEPLOYMENT_TARGET=14.0

say "Tools"
xcode-select -p >/dev/null 2>&1 || { xcode-select --install || true; die "Install the command line tools in the window that opened, then run this again."; }
if ! command -v cargo >/dev/null; then
    [ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"
fi
command -v cargo >/dev/null || die 'Rust is needed: curl -fsSL https://sh.rustup.rs | sh -s -- -y --profile minimal'
cargo --version
swift --version 2>/dev/null | head -1

say "Babel's Rust side (crates/babel-mac)"
(cd "$REPO" && cargo build --release -p babel-mac)

say "The app (macos/)"
(cd "$REPO/macos" && swift build -c release --product Babel)
BIN=$(cd "$REPO/macos" && swift build -c release --show-bin-path)/Babel

say "Babel.app"
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' "$REPO/Cargo.toml" | head -1)
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$BIN" "$APP/Contents/MacOS/Babel"
printf 'APPL????' > "$APP/Contents/PkgInfo"
sed "s/@VERSION@/$VERSION/g" "$REPO/packaging/macos/Info.plist" > "$APP/Contents/Info.plist"

# The icon, from the app's SVG with the margin macOS icons have.
ICONS="$OUT/icon"
rm -rf "$ICONS" && mkdir -p "$ICONS/Babel.iconset"
sed 's/viewBox="0 0 32 32"/viewBox="-3.9 -3.9 39.8 39.8"/' "$REPO/packaging/se.cptjanst.babel.svg" > "$ICONS/babel.svg"
qlmanage -t -s 1024 -o "$ICONS" "$ICONS/babel.svg" >/dev/null 2>&1 || true
if [ -f "$ICONS/babel.svg.png" ]; then
    for s in 16 32 128 256 512; do
        sips -z $s $s "$ICONS/babel.svg.png" --out "$ICONS/Babel.iconset/icon_${s}x${s}.png" >/dev/null
        sips -z $((s * 2)) $((s * 2)) "$ICONS/babel.svg.png" --out "$ICONS/Babel.iconset/icon_${s}x${s}@2x.png" >/dev/null
    done
    iconutil -c icns "$ICONS/Babel.iconset" -o "$APP/Contents/Resources/Babel.icns"
else
    echo "(No icon: Quick Look could not draw the SVG.)"
fi

# Signed for this Mac (ad hoc), so it opens without a warning here.
codesign --force --sign - --timestamp=none "$APP" >/dev/null
echo "$APP"

if [ "$INSTALL" = 1 ]; then
    say "Installing"
    DEST=/Applications
    [ -w "$DEST" ] || DEST="$HOME/Applications"
    mkdir -p "$DEST"
    rm -rf "$DEST/Babel.app"
    cp -R "$APP" "$DEST/"
    /System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister -f "$DEST/Babel.app" 2>/dev/null || true
    echo "$DEST/Babel.app"
fi
