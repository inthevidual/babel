#!/usr/bin/env bash
# Builds Babel for the desktop on a Mac (Apple silicon, or Intel) from this
# repository, as Babel.app. Run it from a clone:
#
#   git clone -b native https://github.com/inthevidual/babel.git
#   cd babel
#   packaging/macos/build-macos.sh --install
#
# Options:
#   --install   also put Babel.app in /Applications (or ~/Applications)
#   --clean     build KDE's Kirigami again even if it is already built
#
# What it needs and gets for you: Xcode's command line tools (it asks you to
# install them if they're missing), Homebrew (it tells you how, if missing),
# Qt and the build tools from Homebrew, and Rust (rustup). Kirigami, the KDE
# toolkit Babel's window is made with, isn't in Homebrew: it is built from
# KDE's source against Homebrew's Qt and kept in ~/.local/babel-kde, with
# KDE's Breeze icons. The first run takes a while; later runs only build Babel.
#
# Babel.app uses Homebrew's Qt where it is installed: it runs on this Mac.
# After `brew upgrade` has given you a new Qt, run this again (it notices,
# and builds Kirigami again to match).
#
# Babel keeps its translations in ~/.local/share/babel, as on Linux.

set -euo pipefail

INSTALL=0
CLEAN=0
for a in "$@"; do
    case "$a" in
        --install) INSTALL=1 ;;
        --clean) CLEAN=1 ;;
        -h|--help) sed -n '2,/^$/p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
        *) echo "Unknown option: $a (see --help)" >&2; exit 2 ;;
    esac
done

say() { printf '\n\033[1m== %s\033[0m\n' "$*"; }
die() { printf '\n\033[1;31m%s\033[0m\n' "$*" >&2; exit 1; }

[ "$(uname -s)" = Darwin ] || die "This script is for macOS."
REPO=$(cd "$(dirname "$0")/../.." && pwd)
[ -f "$REPO/Cargo.toml" ] && [ -d "$REPO/crates/babel-desktop" ] || die "Run it from a clone of Babel (packaging/macos/build-macos.sh)."
[ "$(uname -m)" = arm64 ] || echo "Not Apple silicon ($(uname -m)): building for this Mac anyway."

KDE="$HOME/.local/babel-kde"
WORK="$REPO/target/macos"
mkdir -p "$WORK"

# ── Tools ────────────────────────────────────────────────────────────────

say "Xcode command line tools"
if ! xcode-select -p >/dev/null 2>&1; then
    xcode-select --install || true
    die "Install the command line tools in the window that opened, then run this again."
fi
echo "OK"

say "Homebrew"
if ! command -v brew >/dev/null; then
    for b in /opt/homebrew/bin/brew /usr/local/bin/brew; do
        [ -x "$b" ] && eval "$("$b" shellenv)" && break
    done
fi
command -v brew >/dev/null || die 'Homebrew is needed. Install it with
  /bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)"
then run this again.'
BREW=$(brew --prefix)
echo "$BREW"

say "Qt and build tools (Homebrew)"
# Only the parts of Qt Babel and Kirigami use (not the whole of Qt).
brew install qtbase qtdeclarative qtsvg qtshadertools cmake ninja
QMAKE="$(brew --prefix qtbase)/bin/qmake"
QT_VERSION=$("$QMAKE" -query QT_VERSION)
echo "Qt $QT_VERSION"

say "Rust"
if ! command -v cargo >/dev/null; then
    [ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"
fi
if ! command -v cargo >/dev/null; then
    curl -fsSL https://sh.rustup.rs | sh -s -- -y --profile minimal
    . "$HOME/.cargo/env"
fi
cargo --version

# ── Kirigami (KDE), built against this Qt ────────────────────────────────

newer_or_same() { [ "$(printf '%s\n%s\n' "$1" "$2" | sort -V | tail -1)" = "$1" ]; }

STAMP="$KDE/.built"
if [ "$CLEAN" = 1 ] || [ ! -f "$STAMP" ] || [ "$(sed -n 's/^qt=//p' "$STAMP")" != "$QT_VERSION" ]; then
    say "Kirigami: the newest release this Qt ($QT_VERSION) can build"
    KF=""
    for tag in $(git ls-remote --tags https://invent.kde.org/frameworks/kirigami.git 'v6.*' | awk -F/ '{print $3}' | grep -E '^v6\.[0-9]+\.[0-9]+$' | sort -Vr | head -12); do
        need=$(curl -fsSL "https://invent.kde.org/frameworks/kirigami/-/raw/$tag/CMakeLists.txt" | sed -n 's/^set(REQUIRED_QT_VERSION \([0-9.]*\)).*/\1/p' | head -1)
        if [ -n "$need" ] && newer_or_same "$QT_VERSION" "$need"; then
            KF=${tag#v}
            break
        fi
    done
    [ -n "$KF" ] || die "Found no Kirigami release that builds with Qt $QT_VERSION."
    MAJMIN=${KF%.*}
    echo "Kirigami $KF"

    SRC="$WORK/kde-src"
    rm -rf "$SRC" "$KDE"
    mkdir -p "$SRC" "$KDE"
    for name in extra-cmake-modules kirigami breeze-icons; do
        echo "Downloading $name $KF"
        curl -fsSL "https://download.kde.org/stable/frameworks/$MAJMIN/$name-$KF.tar.xz" | tar -xJ -C "$SRC"
    done

    say "Building KDE's CMake modules"
    cmake -S "$SRC/extra-cmake-modules-$KF" -B "$SRC/build-ecm" -G Ninja \
        -DCMAKE_INSTALL_PREFIX="$KDE" -DBUILD_TESTING=OFF \
        -DBUILD_HTML_DOCS=OFF -DBUILD_MAN_DOCS=OFF -DBUILD_QTHELP_DOCS=OFF >/dev/null
    cmake --build "$SRC/build-ecm" >/dev/null
    cmake --install "$SRC/build-ecm" >/dev/null

    say "Building Kirigami"
    cmake -S "$SRC/kirigami-$KF" -B "$SRC/build-kirigami" -G Ninja \
        -DCMAKE_BUILD_TYPE=Release \
        -DCMAKE_PREFIX_PATH="$KDE;$BREW" \
        -DCMAKE_INSTALL_PREFIX="$KDE" \
        -DCMAKE_INSTALL_NAME_DIR="$KDE/lib" \
        -DKDE_INSTALL_QMLDIR=lib/qml \
        -DBUILD_TESTING=OFF -DBUILD_EXAMPLES=OFF
    cmake --build "$SRC/build-kirigami"
    cmake --install "$SRC/build-kirigami" >/dev/null

    say "Breeze icons"
    icons="$SRC/breeze-icons-$KF/icons"
    mkdir -p "$KDE/share/icons"
    cp -R "$icons" "$KDE/share/icons/breeze"
    # The theme's index, as KDE's build makes it.
    cat "$icons/breeze.theme.in" "$icons/commonthemeinfo.theme.in" > "$KDE/share/icons/breeze/index.theme"
    rm -f "$KDE/share/icons/breeze/"*.in "$KDE/share/icons/breeze/"*.cpp "$KDE/share/icons/breeze/"*.h "$KDE/share/icons/breeze/CMakeLists.txt"

    printf 'kf=%s\nqt=%s\n' "$KF" "$QT_VERSION" > "$STAMP"
    rm -rf "$SRC"
else
    echo
    echo "Kirigami $(sed -n 's/^kf=//p' "$STAMP") for Qt $QT_VERSION: already built ($KDE)."
fi
[ -d "$KDE/lib/qml/org/kde/kirigami" ] || die "Kirigami's QML module is missing from $KDE/lib/qml (try --clean)."

# ── Babel ────────────────────────────────────────────────────────────────

say "Building Babel"
cd "$REPO"
QMAKE="$QMAKE" cargo build --release -p babel-desktop

say "Babel.app"
APP="$WORK/Babel.app"
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' "$REPO/Cargo.toml" | head -1)
COMMIT=$(git -C "$REPO" rev-parse --short HEAD 2>/dev/null || echo unknown)
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources/icons"
cp "$REPO/target/release/babel-desktop" "$APP/Contents/MacOS/babel-desktop"
cp -R "$KDE/share/icons/breeze" "$APP/Contents/Resources/icons/breeze"

# Starts Babel with Kirigami where this script built it.
cat > "$APP/Contents/MacOS/Babel" <<EOF
#!/bin/sh
here=\$(cd "\$(dirname "\$0")" && pwd)
export QML_IMPORT_PATH="$KDE/lib/qml\${QML_IMPORT_PATH:+:\$QML_IMPORT_PATH}"
exec "\$here/babel-desktop" "\$@"
EOF
chmod 755 "$APP/Contents/MacOS/Babel"

cat > "$APP/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key><string>Babel</string>
    <key>CFBundleDisplayName</key><string>Babel</string>
    <key>CFBundleIdentifier</key><string>se.cptjanst.babel</string>
    <key>CFBundleExecutable</key><string>Babel</string>
    <key>CFBundleIconFile</key><string>babel</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleShortVersionString</key><string>$VERSION</string>
    <key>CFBundleVersion</key><string>$COMMIT</string>
    <key>LSMinimumSystemVersion</key><string>13.0</string>
    <key>NSHighResolutionCapable</key><true/>
    <key>NSSupportsAutomaticGraphicsSwitching</key><true/>
</dict>
</plist>
EOF

# The icon, from Babel's SVG (Quick Look draws it; skipped if that fails).
ICONSET="$WORK/babel.iconset"
rm -rf "$ICONSET" && mkdir -p "$ICONSET"
if qlmanage -t -s 1024 -o "$WORK" "$REPO/packaging/se.cptjanst.babel.svg" >/dev/null 2>&1 && [ -f "$WORK/se.cptjanst.babel.svg.png" ]; then
    for s in 16 32 128 256 512; do
        sips -z $s $s "$WORK/se.cptjanst.babel.svg.png" --out "$ICONSET/icon_${s}x${s}.png" >/dev/null
        sips -z $((s * 2)) $((s * 2)) "$WORK/se.cptjanst.babel.svg.png" --out "$ICONSET/icon_${s}x${s}@2x.png" >/dev/null
    done
    iconutil -c icns "$ICONSET" -o "$APP/Contents/Resources/babel.icns" || true
fi
rm -rf "$ICONSET" "$WORK/se.cptjanst.babel.svg.png"

# Signed for this Mac (ad hoc), as Apple silicon wants every app to be.
codesign --force --deep --sign - "$APP" >/dev/null 2>&1 || echo "(Could not sign the app ad hoc; it may still open.)"

if [ "$INSTALL" = 1 ]; then
    DEST=/Applications
    [ -w "$DEST" ] || { DEST="$HOME/Applications"; mkdir -p "$DEST"; }
    say "Installing in $DEST"
    rm -rf "$DEST/Babel.app"
    cp -R "$APP" "$DEST/Babel.app"
    APP="$DEST/Babel.app"
fi

cat <<EOF

Done: $APP
Open it from Finder, or:  open "$APP"
After a Homebrew upgrade of Qt, run this script again.
EOF
