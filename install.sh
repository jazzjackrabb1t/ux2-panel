#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Builds ux2ctl and installs it with its udev rule, its settings file and the
# ux2-panel meter service.
#
#   ./install.sh                     build here, then install (asks for sudo)
#   ./install.sh --binary PATH       install a binary built elsewhere
#   ./install.sh --no-service        leave the meter service off
#   ./install.sh --prefix /usr       install under another prefix
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
prefix=/usr/local
binary=""
service=1

while [ $# -gt 0 ]; do
    case "$1" in
        --binary) binary="$2"; shift 2 ;;
        --prefix) prefix="$2"; shift 2 ;;
        --no-service) service=0; shift ;;
        -h | --help) sed -n '3,9p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
        *) echo "install.sh: unknown option $1" >&2; exit 2 ;;
    esac
done

as_root() {
    if [ "$(id -u)" -eq 0 ]; then "$@"; else sudo "$@"; fi
}

if [ -z "$binary" ]; then
    cargo build --release --locked -p ux2ctl --manifest-path "$here/Cargo.toml"
    binary="$here/target/release/ux2ctl"
fi

as_root install -Dm755 "$binary" "$prefix/bin/ux2ctl"

as_root install -Dm644 "$here/packaging/70-ux2-panel.rules" /etc/udev/rules.d/70-ux2-panel.rules
as_root udevadm control --reload
as_root udevadm trigger --action=change --subsystem-match=usb --attr-match=idVendor=0e41

if [ ! -e /etc/ux2-panel.toml ]; then
    as_root install -Dm644 "$here/packaging/ux2-panel.toml" /etc/ux2-panel.toml
fi

if ! getent passwd ux2-panel >/dev/null; then
    as_root useradd --system --gid audio --no-create-home --home-dir /nonexistent \
        --shell /usr/sbin/nologin ux2-panel
fi

sed "s|/usr/local/bin/ux2ctl|$prefix/bin/ux2ctl|" "$here/packaging/ux2-panel.service" \
    | as_root tee /etc/systemd/system/ux2-panel.service >/dev/null
as_root systemctl daemon-reload
if [ "$service" -eq 1 ]; then
    as_root systemctl enable --now ux2-panel.service
    as_root systemctl restart ux2-panel.service
fi

echo "Installed $prefix/bin/ux2ctl. Add yourself to group audio to drive the panel"
echo "without sudo, then check it with: ux2ctl status"
