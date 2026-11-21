#!/usr/bin/env bash
# Runs the macOS app's UI tests (macos/tests/*.json) through real AppKit
# events, in a data folder of their own; the sync tests against a local
# babel-server. Build first: packaging/macos/build-app.sh.
#
#   packaging/macos/test-app.sh              # screenshots in target/macos/uitest/
#
# The window comes up while they run; keep your hands off the keyboard.

set -euo pipefail

REPO=$(cd "$(dirname "$0")/../.." && pwd)
APP="$REPO/target/macos/Babel.app/Contents/MacOS/Babel"
OUT="$REPO/target/macos/uitest"
[ -x "$APP" ] || { echo "Build the app first: packaging/macos/build-app.sh" >&2; exit 1; }
[ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"

rm -rf "$OUT" && mkdir -p "$OUT/data" "$OUT/data2"
PORT=8799
SERVER="http://127.0.0.1:$PORT"
failed=0

run() { # script data-dir
    local script="$OUT/$(basename "$1")"
    sed -e "s|@REPO@|$REPO|g" -e "s|@OUT@|$OUT|g" -e "s|@SERVER@|$SERVER|g" "$1" > "$script"
    echo "== $(basename "$1")"
    local log
    log=$(BABEL_DATA_DIR="$2" BABEL_TEST_SCRIPT="$script" "$APP" 2>/dev/null | grep '^UITEST' || true)
    echo "$log" | grep -E '^UITEST (ok|FAIL)' | sed 's/^UITEST //'
    echo "$log" | grep -q '^UITEST done, 0 failed' || failed=1
}

run "$REPO/macos/tests/editing.json" "$OUT/data"
run "$REPO/macos/tests/projects.json" "$OUT/data"

(cd "$REPO" && cargo build --release -q -p babel-server)
echo testpass123 | "$REPO/target/release/babel-server" --db "$OUT/server.db" user add tester >/dev/null
"$REPO/target/release/babel-server" --db "$OUT/server.db" serve --listen "127.0.0.1:$PORT" >"$OUT/server.log" 2>&1 &
SERVER_PID=$!
trap 'kill $SERVER_PID 2>/dev/null || true' EXIT
sleep 1
run "$REPO/macos/tests/sync.json" "$OUT/data"
run "$REPO/macos/tests/sync-second.json" "$OUT/data2"

if [ "$failed" = 0 ]; then echo "All passed. Screenshots: $OUT"; else echo "Some failed. Screenshots: $OUT"; exit 1; fi
