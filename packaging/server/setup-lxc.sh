#!/usr/bin/env bash
# Installs, or updates, the Babel server (sync service and web app) in a
# Debian or Ubuntu container, such as a Proxmox LXC, to run behind Nginx
# Proxy Manager.
#
# Run it as root inside the container:
#
#   curl -fsSLO https://raw.githubusercontent.com/inthevidual/babel/native/packaging/server/setup-lxc.sh
#   bash setup-lxc.sh --user jesper
#
# Run it again to update to the newest commit. Options:
#
#   --user NAME     create this account (the password is asked for)
#   --branch NAME   the git branch to build (default: native)
#   --port PORT     the port Nginx Proxy Manager forwards to (default: 8790)
#   --repo URL      the git repository (default: Babel's on GitHub)
#
# Babel is built from source here, so the binary matches the container's
# system. The first build takes a while and wants about 2 GB of memory
# (memory and swap together).
#
# What it sets up:
#   /opt/babel/src           the source, and the Rust toolchain in /opt/babel/rust
#   /usr/local/bin/babel-server
#   /usr/local/bin/babel-admin   accounts: babel-admin user add|passwd|del|list
#   /etc/babel/babel.env     settings (kept when you run this again)
#   /var/lib/babel/babel.db  the translations: the one file to back up
#   babel.service            systemd, as user "babel"

set -euo pipefail

REPO=https://github.com/inthevidual/babel.git
BRANCH=native
PORT=8790
NEW_USER=

while [ $# -gt 0 ]; do
    case "$1" in
        --user) NEW_USER=${2:?--user needs a name}; shift 2 ;;
        --branch) BRANCH=${2:?--branch needs a name}; shift 2 ;;
        --port) PORT=${2:?--port needs a number}; shift 2 ;;
        --repo) REPO=${2:?--repo needs a URL}; shift 2 ;;
        -h|--help) sed -n '2,/^$/p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
        *) echo "Unknown option: $1 (see --help)" >&2; exit 2 ;;
    esac
done

say() { printf '\n\033[1m== %s\033[0m\n' "$*"; }

[ "$(id -u)" = 0 ] || { echo "Run this as root." >&2; exit 1; }
command -v apt-get >/dev/null || { echo "This script is for Debian or Ubuntu." >&2; exit 1; }
command -v systemctl >/dev/null || { echo "systemd is needed." >&2; exit 1; }

BASE=/opt/babel
SRC=$BASE/src
export RUSTUP_HOME=$BASE/rust CARGO_HOME=$BASE/rust
export PATH=$CARGO_HOME/bin:$PATH
DATA=/var/lib/babel
ENV=/etc/babel/babel.env

say "Packages"
export DEBIAN_FRONTEND=noninteractive
apt-get update -q
apt-get install -y -q --no-install-recommends ca-certificates curl git build-essential pkg-config

say "Rust"
if ! command -v rustup >/dev/null; then
    curl -fsSL https://sh.rustup.rs | sh -s -- -y -q --no-modify-path --profile minimal --default-toolchain stable
else
    rustup update stable --no-self-update
fi
rustup target add wasm32-unknown-unknown

say "Source ($BRANCH)"
if [ -d "$SRC/.git" ]; then
    git -C "$SRC" remote set-url origin "$REPO"
    git -C "$SRC" fetch -q origin "$BRANCH"
    git -C "$SRC" checkout -q -B "$BRANCH" "origin/$BRANCH"
    git -C "$SRC" reset -q --hard "origin/$BRANCH"
else
    git clone -q --branch "$BRANCH" "$REPO" "$SRC"
fi
COMMIT=$(git -C "$SRC" rev-parse --short HEAD)
echo "At $COMMIT."

# wasm-bindgen's command must be the version the editor is built with.
WB=$(awk '/^name = "wasm-bindgen"$/ { getline; gsub(/[^0-9.]/, ""); print; exit }' "$SRC/Cargo.lock")
if [ "$(wasm-bindgen --version 2>/dev/null | awk '{print $2}')" != "$WB" ]; then
    say "wasm-bindgen $WB"
    cargo install -q --locked wasm-bindgen-cli --version "$WB"
fi

say "Building (the first time takes a while)"
make -C "$SRC" server
install -m 755 "$SRC/target/release/babel-server" /usr/local/bin/babel-server.new
mv -f /usr/local/bin/babel-server.new /usr/local/bin/babel-server

say "Service"
id babel >/dev/null 2>&1 || useradd --system --home-dir "$DATA" --shell /usr/sbin/nologin babel
install -d -o babel -g babel -m 750 "$DATA"
install -d -m 755 /etc/babel
if [ ! -f "$ENV" ]; then
    cat > "$ENV" <<EOF
# Babel's settings. Restart after a change: systemctl restart babel
BABEL_DB=$DATA/babel.db
# 0.0.0.0: reachable from Nginx Proxy Manager on another machine.
BABEL_LISTEN=0.0.0.0:$PORT
# Nginx Proxy Manager serves Babel over HTTPS: the session cookie is Secure.
# Set to false only to try Babel over plain http.
BABEL_SECURE_COOKIES=true
RUST_LOG=info,tower_http=warn
EOF
else
    echo "Keeping $ENV."
fi

cat > /etc/systemd/system/babel.service <<'EOF'
[Unit]
Description=Babel translation server (sync and web app)
After=network-online.target
Wants=network-online.target

[Service]
User=babel
Group=babel
EnvironmentFile=/etc/babel/babel.env
WorkingDirectory=/var/lib/babel
ExecStart=/usr/local/bin/babel-server serve
Restart=on-failure
RestartSec=3
# Open event streams (the web app's live updates) don't end by themselves.
TimeoutStopSec=10
NoNewPrivileges=yes
UMask=0027

[Install]
WantedBy=multi-user.target
EOF

cat > /usr/local/bin/babel-admin <<'EOF'
#!/bin/sh
# Babel's accounts, on the server's database: babel-admin user add|passwd|del|list NAME
set -a; . /etc/babel/babel.env; set +a
exec runuser -u babel -- /usr/local/bin/babel-server --db "$BABEL_DB" "$@"
EOF
chmod 755 /usr/local/bin/babel-admin

systemctl daemon-reload
systemctl enable -q babel
systemctl restart babel

say "Checking"
LISTEN=$(sed -n 's/^BABEL_LISTEN=//p' "$ENV")
LPORT=${LISTEN##*:}
for _ in $(seq 1 20); do
    curl -fsS "http://127.0.0.1:$LPORT/api/config" >/dev/null 2>&1 && break
    sleep 0.5
done
if curl -fsS "http://127.0.0.1:$LPORT/api/config" >/dev/null 2>&1 && curl -fsS "http://127.0.0.1:$LPORT/" | grep -qi "<html"; then
    echo "Babel $COMMIT is running on port $LPORT."
else
    echo "Babel did not answer. Its log:" >&2
    journalctl -u babel -n 30 --no-pager >&2
    exit 1
fi

if [ -n "$NEW_USER" ]; then
    say "Account $NEW_USER"
    if babel-admin user list | grep -qx "$NEW_USER"; then
        echo "$NEW_USER already exists (change the password: babel-admin user passwd $NEW_USER)."
    else
        babel-admin user add "$NEW_USER"
    fi
fi

IP=$(hostname -I 2>/dev/null | awk '{print $1}')
[ -n "$IP" ] || IP="(this container's address)"
cat <<EOF

Done. In Nginx Proxy Manager, add a Proxy Host:
  Domain Names:          your Babel address, e.g. babel.example.org
  Scheme:                http
  Forward Hostname / IP: $IP
  Forward Port:          $LPORT
  Block Common Exploits: on
  SSL:                   request a certificate; Force SSL and HTTP/2 on
No custom nginx settings are needed.

Then open https://<that address>, or sign in from the desktop app
(Sign In to Sync…) with the same address.

Accounts:  babel-admin user add NAME   (also passwd, del, list)
Log:       journalctl -u babel -f
Update:    run this script again
Back up:   $DATA/babel.db (a Proxmox backup of the container covers it)
EOF
if [ "$(babel-admin user list | wc -l)" = 0 ]; then
    echo
    echo "There are no accounts yet: babel-admin user add NAME"
fi
