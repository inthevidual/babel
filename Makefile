# Builds and installs the desktop app for the current user (or PREFIX=/usr
# with sudo), and the server binary.

PREFIX ?= $(HOME)/.local
BIN = target/release

.PHONY: all desktop server wasm install install-server test uninstall

all: desktop server

desktop: wasm
	cargo build --release -p babel-desktop

server: wasm
	cargo build --release -p babel-server

# The editor for the browser: web/wasm/ (needs rust-wasm and wasm-bindgen).
wasm:
	cargo build --profile wasm -p babel-wasm --target wasm32-unknown-unknown
	wasm-bindgen --target web --no-typescript --out-dir web/wasm target/wasm32-unknown-unknown/wasm/babel_wasm.wasm

install: desktop
	install -Dm755 $(BIN)/babel-desktop $(PREFIX)/bin/babel-desktop
	install -Dm644 packaging/se.cptjanst.babel.desktop $(PREFIX)/share/applications/se.cptjanst.babel.desktop
	install -Dm644 packaging/se.cptjanst.babel.svg $(PREFIX)/share/icons/hicolor/scalable/apps/se.cptjanst.babel.svg
	-update-desktop-database $(PREFIX)/share/applications 2>/dev/null

install-server: server
	install -Dm755 $(BIN)/babel-server $(PREFIX)/bin/babel-server

uninstall:
	rm -f $(PREFIX)/bin/babel-desktop $(PREFIX)/share/applications/se.cptjanst.babel.desktop \
	  $(PREFIX)/share/icons/hicolor/scalable/apps/se.cptjanst.babel.svg

test:
	cargo test --workspace
