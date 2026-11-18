#!/usr/bin/env bash
# Creates a Debian LXC container on this Proxmox host and installs the Babel
# server in it (setup-lxc.sh). Run as root on the Proxmox host:
#
#   bash -c "$(curl -fsSL https://raw.githubusercontent.com/inthevidual/babel/native/packaging/server/proxmox-create.sh)"
#
# Settings, as environment variables before the command (defaults in brackets):
#   CTID        container id [the next free one]
#   CT_NAME     host name [babel]
#   STORAGE     storage for the container's disk [local-lvm]
#   TPL_STORAGE storage for the template [local]
#   BRIDGE      network bridge [vmbr0]
#   IP          address: dhcp, or e.g. 192.168.1.50/24 [dhcp]
#   GATEWAY     gateway, with a fixed IP
#   BABEL_USER  the first account [asked for]
#   BRANCH      the git branch [native]

set -euo pipefail

CTID=${CTID:-}
CT_NAME=${CT_NAME:-babel}
STORAGE=${STORAGE:-local-lvm}
TPL_STORAGE=${TPL_STORAGE:-local}
BRIDGE=${BRIDGE:-vmbr0}
IP=${IP:-dhcp}
GATEWAY=${GATEWAY:-}
BRANCH=${BRANCH:-native}
RAW=https://raw.githubusercontent.com/inthevidual/babel/$BRANCH/packaging/server/setup-lxc.sh

say() { printf '\n\033[1m== %s\033[0m\n' "$*"; }

[ "$(id -u)" = 0 ] || { echo "Run this as root on the Proxmox host." >&2; exit 1; }
command -v pct >/dev/null || { echo "pct not found: run this on the Proxmox host." >&2; exit 1; }

# The first account, asked for now so the rest runs unattended.
BABEL_USER=${BABEL_USER:-}
if [ -z "$BABEL_USER" ]; then
    read -r -p "Babel account to create (e.g. your name): " BABEL_USER
fi
[ -n "$BABEL_USER" ] || { echo "An account name is needed." >&2; exit 1; }
while :; do
    read -r -s -p "Password for $BABEL_USER: " PW; echo
    read -r -s -p "Again: " PW2; echo
    [ -n "$PW" ] && [ "$PW" = "$PW2" ] && break
    echo "Empty, or the two differ. Again."
done
unset PW2

say "Template"
pveam update >/dev/null
# The host's architecture: the list can hold the same template for several.
case "$(uname -m)" in
    x86_64) ARCH=amd64 ;;
    aarch64) ARCH=arm64 ;;
    *) ARCH=$(dpkg --print-architecture) ;;
esac
pick() { pveam available --section system | awk '{print $2}' | grep -E "^$1-standard_.*_${ARCH}\.tar\." | sort -V | tail -1; }
TPL=$(pick debian-13)
[ -n "$TPL" ] || TPL=$(pick debian-12)
[ -n "$TPL" ] || { echo "No Debian template found (pveam available)." >&2; exit 1; }
if ! pveam list "$TPL_STORAGE" | grep -q "$TPL"; then
    pveam download "$TPL_STORAGE" "$TPL"
fi
echo "$TPL"

say "Container"
CTID=${CTID:-$(pvesh get /cluster/nextid)}
NET="name=eth0,bridge=$BRIDGE,ip=$IP"
[ -n "$GATEWAY" ] && NET="$NET,gw=$GATEWAY"
pct create "$CTID" "$TPL_STORAGE:vztmpl/$TPL" \
    --hostname "$CT_NAME" --cores 2 --memory 2048 --swap 1024 \
    --rootfs "$STORAGE:8" --net0 "$NET" \
    --unprivileged 1 --features nesting=1 --onboot 1
if ! pct start "$CTID" || [ "$(pct status "$CTID" | awk '{print $2}')" != running ]; then
    echo >&2
    echo "Container $CTID was created but does not start. Proxmox's reason:" >&2
    pct start "$CTID" --debug 2>&1 | grep -iE 'error|fail|denied|unsupported|cannot|no such' | tail -15 >&2 || true
    echo "Full log: pct start $CTID --debug    Remove it: pct destroy $CTID" >&2
    exit 1
fi
echo "Container $CTID ($CT_NAME) is running."

# Wait for its network (DHCP) before installing.
for _ in $(seq 1 60); do
    pct exec "$CTID" -- getent hosts deb.debian.org >/dev/null 2>&1 && break
    sleep 2
done
pct exec "$CTID" -- getent hosts deb.debian.org >/dev/null 2>&1 || { echo "Container $CTID has no network." >&2; exit 1; }

say "Babel (building takes a while)"
TMP=$(mktemp)
trap 'rm -f "$TMP"' EXIT
curl -fsSL "$RAW" -o "$TMP"
pct push "$CTID" "$TMP" /root/setup-lxc.sh --perms 755
pct exec "$CTID" -- bash /root/setup-lxc.sh --branch "$BRANCH" < /dev/null

say "Account $BABEL_USER"
printf '%s\n' "$PW" | pct exec "$CTID" -- /usr/local/bin/babel-admin user add "$BABEL_USER"
unset PW

echo
echo "Container $CTID is ready. Point Nginx Proxy Manager at it as shown above."
echo "To update Babel later:  pct exec $CTID -- bash /root/setup-lxc.sh"
