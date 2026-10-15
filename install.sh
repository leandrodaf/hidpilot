#!/usr/bin/env bash
# Builds hidpilot and installs it for the current user, plus the system parts (udev rules,
# battery service) after asking. Safe to run again: every step overwrites or skips.
#
#   ./install.sh        ask before each system change
#   ./install.sh -y     answer yes to everything
set -euo pipefail

cd "$(dirname "$0")"

YES=0
[[ "${1:-}" == "-y" || "${1:-}" == "--yes" ]] && YES=1

BIN="$HOME/.local/bin"
UNITS="$HOME/.config/systemd/user"
APPS="$HOME/.local/share/applications"
ICONS="$HOME/.local/share/icons/hicolor/scalable/apps"

say() { printf '\033[1m==>\033[0m %s\n' "$*"; }
warn() { printf '\033[33mwarning:\033[0m %s\n' "$*" >&2; }

ask() {
    if [[ $YES == 1 ]]; then return 0; fi
    local reply
    read -r -p "$1 [y/N] " reply
    [[ "$reply" =~ ^[YySs]$ ]]
}

command -v cargo >/dev/null || { echo "cargo not found: install Rust from https://rustup.rs" >&2; exit 1; }

say "Building (release)"
cargo build --release -p hidpilot -p hidpilot-flow
GUI=1
if ! cargo build --release -p hidpilot-gui; then
    GUI=0
    warn "the desktop app did not build (sudo apt install libwebkit2gtk-4.1-dev libgtk-3-dev); continuing without it"
fi

say "Installing programs in $BIN"
install -Dm755 target/release/hidpilot "$BIN/hidpilot"
install -Dm755 target/release/hidpilot-flow "$BIN/hidpilot-flow"
if [[ $GUI == 1 ]]; then
    install -Dm755 target/release/hidpilot-gui "$BIN/hidpilot-gui"
    install -Dm644 packaging/hidpilot.desktop "$APPS/hidpilot.desktop"
    install -Dm644 packaging/hidpilot.svg "$ICONS/hidpilot.svg"
    if command -v update-desktop-database >/dev/null; then
        update-desktop-database -q "$APPS" || true
    fi
fi
case ":$PATH:" in
    *":$BIN:"*) ;;
    *) warn "$BIN is not in your PATH; add it to your shell profile" ;;
esac

say "Installing user services"
install -Dm644 systemd/user/hidpilot-notify.service "$UNITS/hidpilot-notify.service"
install -Dm644 systemd/user/hidpilot-flow.service "$UNITS/hidpilot-flow.service"
install -Dm644 systemd/user/hidpilot-remap.service "$UNITS/hidpilot-remap.service"
systemctl --user daemon-reload
systemctl --user enable --now hidpilot-notify.service
if ask "Start Flow (move mouse and keyboard between computers) at login?"; then
    systemctl --user enable --now hidpilot-flow.service
fi
# Button remapping ships separately; only enable it once the binary knows the command.
if "$BIN/hidpilot" remap --help >/dev/null 2>&1; then
    if ask "Start custom button actions at login?"; then
        systemctl --user enable --now hidpilot-remap.service
    fi
fi

if ask "Install the system parts (device access rules and the battery service)? Needs sudo."; then
    say "Installing udev rules and the battery service"
    sudo install -Dm755 target/release/hidpilot /usr/local/bin/hidpilot
    sudo install -Dm644 udev/70-hidpilot.rules /etc/udev/rules.d/70-hidpilot.rules
    sudo install -Dm644 systemd/system/hidpilot-battery.service /etc/systemd/system/hidpilot-battery.service
    sudo udevadm control --reload
    sudo udevadm trigger --subsystem-match=hidraw --subsystem-match=misc --subsystem-match=input
    sudo systemctl daemon-reload
    sudo systemctl enable --now hidpilot-battery.service
    # Restart picks up a rebuilt binary when this script runs again.
    sudo systemctl restart hidpilot-battery.service
fi

# Flow listens on two TCP ports and finds peers with mDNS. Open them for the local
# network only, and only if a firewall is in the way.
if command -v ufw >/dev/null; then
    iface=$(ip route show default 2>/dev/null | awk '{print $5; exit}')
    subnet=""
    if [[ -n "$iface" ]]; then
        subnet=$(ip -o -4 route show dev "$iface" proto kernel scope link 2>/dev/null | awk '{print $1; exit}')
    fi
    if [[ -n "$subnet" ]] && ask "Allow Flow through the firewall from your local network ($subnet) only? Needs sudo."; then
        sudo ufw allow from "$subnet" to any port 47811:47812 proto tcp comment 'hidpilot flow'
        sudo ufw allow from "$subnet" to any port 5353 proto udp comment 'mDNS (hidpilot flow)'
    fi
fi

say "Done"
echo "  hidpilot list              connected devices"
echo "  hidpilot-flow pair         pair this computer with another one"
[[ $GUI == 1 ]] && echo "  hidpilot-gui               settings app (also in the app grid)"
true
