#!/usr/bin/env bash
# Installs hidpilot: the CLI, the desktop app, the background services and the system
# parts (device access rules, battery service). One confirmation, one sudo prompt.
# Safe to run again; it updates whatever is already there.
#
#   ./install.sh                 install everything
#   ./install.sh -y              same, without the confirmation
#   ./install.sh --no-flow       skip Flow (moving mouse and keyboard between computers)
#   ./install.sh --no-remap      skip custom button and key actions
#   ./install.sh --no-firewall   do not open Flow's ports in ufw
#   ./install.sh --no-assets     do not download Logitech's pictures and fonts (~900 MB)
#
# Everything lives in main(), so editing this file while it runs cannot break the run.
set -euo pipefail

YES=0 FLOW=1 REMAP=1 FIREWALL=1 ASSETS=1

BIN="$HOME/.local/bin"
UNITS="$HOME/.config/systemd/user"
APPS="$HOME/.local/share/applications"
HICOLOR="$HOME/.local/share/icons/hicolor"
ASSET_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/hidpilot/logitech"

bold() { printf '\033[1m%s\033[0m\n' "$*"; }
step() { printf '\033[1;34m==>\033[0m %s\n' "$*"; }
ok() { printf '  \033[32m✓\033[0m %s\n' "$*"; }
bad() { printf '  \033[31m✗\033[0m %s\n' "$*"; }
note() { printf '  \033[33m!\033[0m %s\n' "$*"; }
die() { printf '\033[31merror:\033[0m %s\n' "$*" >&2; exit 1; }

parse_args() {
    for a in "$@"; do
        case "$a" in
            -y|--yes) YES=1 ;;
            --no-flow) FLOW=0 ;;
            --no-remap) REMAP=0 ;;
            --no-firewall) FIREWALL=0 ;;
            --no-assets) ASSETS=0 ;;
            -h|--help) sed -n '2,13p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
            *) die "unknown option $a (see --help)" ;;
        esac
    done
}

check_requirements() {
    command -v cargo >/dev/null || die "Rust is not installed. Install it with: curl https://sh.rustup.rs -sSf | sh"
    local missing=()
    pkg-config --exists webkit2gtk-4.1 2>/dev/null || missing+=(libwebkit2gtk-4.1-dev)
    pkg-config --exists gtk+-3.0 2>/dev/null || missing+=(libgtk-3-dev)
    # the tray icon loads this library at run time
    ldconfig -p 2>/dev/null | grep -q libayatana-appindicator3 || missing+=(libayatana-appindicator3-1)
    command -v 7z >/dev/null || missing+=(p7zip-full)
    if ((${#missing[@]})); then
        die "missing system packages. Install them with:
    sudo apt install ${missing[*]}"
    fi
}

subnet() {
    local iface
    iface=$(ip route show default 2>/dev/null | awk '{print $5; exit}')
    [[ -n "$iface" ]] && ip -o -4 route show dev "$iface" proto kernel scope link 2>/dev/null | awk '{print $1; exit}'
}

plan() {
    bold "hidpilot will install:"
    echo "  • hidpilot, hidpilot-flow and the Orbit app in $BIN (and in the app grid)"
    echo "  • battery service: GNOME shows your mouse, keyboard and headset batteries   (sudo)"
    echo "  • device access rules, so no sudo is needed afterwards                     (sudo)"
    echo "  • low battery notifications"
    ((REMAP)) && echo "  • custom button and key actions (gestures, per-app profiles)"
    ((FLOW)) && echo "  • Flow: move mouse and keyboard between computers"
    if ((FLOW && FIREWALL)) && command -v ufw >/dev/null && [[ -n "$(subnet)" ]]; then
        echo "  • firewall: open Flow only to your local network $(subnet)                 (sudo)"
    fi
    if ((ASSETS)) && [[ ! -f "$ASSET_DIR/devices/mx-master-3s.webp" ]]; then
        echo "  • Logitech's device pictures and fonts, from Logitech's installers (~900 MB download)"
    fi
    echo
    if ((!YES)); then
        local reply
        read -r -p "Continue? [Y/n] " reply
        [[ -z "$reply" || "$reply" =~ ^[YySs]$ ]] || exit 0
    fi
}

build() {
    step "Building (the first time takes a few minutes)"
    cargo build --release --workspace --quiet
    ok "built"
}

install_user_parts() {
    step "Installing programs"
    install -Dm755 target/release/hidpilot "$BIN/hidpilot"
    install -Dm755 target/release/hidpilot-flow "$BIN/hidpilot-flow"
    install -Dm755 target/release/hidpilot-gui "$BIN/hidpilot-gui"
    install -Dm644 packaging/hidpilot.desktop "$APPS/hidpilot.desktop"
    install -Dm644 packaging/hidpilot.svg "$HICOLOR/scalable/apps/hidpilot.svg"
    for size in 48 64 128 256; do
        install -Dm644 "packaging/icons/hidpilot-$size.png" "$HICOLOR/${size}x${size}/apps/hidpilot.png"
    done
    # A stale user icon cache hides new icons.
    command -v gtk-update-icon-cache >/dev/null && gtk-update-icon-cache -q -f -t "$HICOLOR" || true
    command -v update-desktop-database >/dev/null && update-desktop-database -q "$APPS" || true
    ok "programs in $BIN"
    case ":$PATH:" in
        *":$BIN:"*) ;;
        *) note "$BIN is not in your PATH; open a new terminal or add it to ~/.profile" ;;
    esac

    step "Starting background services"
    for unit in hidpilot-notify hidpilot-flow hidpilot-remap; do
        install -Dm644 "systemd/user/$unit.service" "$UNITS/$unit.service"
    done
    systemctl --user daemon-reload
    enable_user hidpilot-notify
    if ((REMAP)); then enable_user hidpilot-remap; else systemctl --user disable --now hidpilot-remap.service 2>/dev/null || true; fi
    if ((FLOW)); then enable_user hidpilot-flow; else systemctl --user disable --now hidpilot-flow.service 2>/dev/null || true; fi
}

enable_user() {
    systemctl --user enable "$1.service" --quiet 2>/dev/null || true
    # restart picks up a rebuilt binary when the script runs again
    systemctl --user restart "$1.service"
}

# Ask for the password once, up front, and keep sudo alive until the script ends, so a
# long build never leaves the user typing it again halfway.
sudo_once() {
    step "Asking for your password once (for the battery service and device rules)"
    sudo -v || die "sudo is needed for the battery service and device rules"
    while kill -0 "$$" 2>/dev/null; do
        sudo -n true 2>/dev/null
        sleep 50
    done &
}

install_system_parts() {
    step "Installing system parts"
    sudo install -Dm755 target/release/hidpilot /usr/local/bin/hidpilot
    sudo install -Dm644 udev/70-hidpilot.rules /etc/udev/rules.d/70-hidpilot.rules
    sudo install -Dm644 systemd/system/hidpilot-battery.service /etc/systemd/system/hidpilot-battery.service
    echo uhid | sudo tee /etc/modules-load.d/hidpilot.conf >/dev/null
    sudo udevadm control --reload
    sudo udevadm trigger --subsystem-match=hidraw --subsystem-match=misc --subsystem-match=input
    sudo systemctl daemon-reload
    sudo systemctl enable --quiet hidpilot-battery.service
    sudo systemctl restart hidpilot-battery.service
    ok "device rules and battery service"

    local net
    net=$(subnet || true)
    if ((FLOW && FIREWALL)) && command -v ufw >/dev/null && [[ -n "$net" ]]; then
        sudo ufw allow from "$net" to any port 47811:47812 proto tcp comment 'hidpilot flow' >/dev/null
        sudo ufw allow from "$net" to any port 5353 proto udp comment 'mDNS (hidpilot flow)' >/dev/null
        ok "firewall open for Flow on $net"
    fi
}

fetch_assets() {
    if ((ASSETS)) && [[ ! -f "$ASSET_DIR/devices/mx-master-3s.webp" ]]; then
        step "Downloading Logitech's pictures and fonts"
        if ./scripts/fetch-logitech-assets.sh; then
            ok "pictures and fonts in $ASSET_DIR"
        else
            note "could not fetch them now; the app works without them. Retry with ./scripts/fetch-logitech-assets.sh"
        fi
    fi
}

verify() {
    step "Checking"
    systemctl is-active --quiet hidpilot-battery && ok "battery service running" || bad "battery service is not running (journalctl -u hidpilot-battery)"
    local units=(hidpilot-notify)
    ((REMAP)) && units+=(hidpilot-remap)
    ((FLOW)) && units+=(hidpilot-flow)
    for u in "${units[@]}"; do
        systemctl --user is-active --quiet "$u" && ok "$u running" || bad "$u is not running (journalctl --user -u $u)"
    done
    # Give the battery service a moment to publish, then ask UPower what GNOME sees.
    local n=0
    for _ in 1 2 3 4 5 6 7 8; do
        n=$(upower -e 2>/dev/null | grep -c hidpilot || true)
        ((n > 0)) && break
        sleep 1
    done
    if ((n > 0)); then
        upower -e | grep hidpilot | while read -r dev; do
            ok "GNOME sees: $(upower -i "$dev" | awk -F': *' '/model:/{m=$2} /percentage:/{p=$2} END{print m" "p}')"
        done
    else
        note "no device batteries yet: switch the devices on, or check: journalctl -u hidpilot-battery"
    fi
    [[ -f "$ASSET_DIR/devices/mx-master-3s.webp" ]] && ok "Logitech pictures installed" || note "no Logitech pictures: the app shows simple drawings (./scripts/fetch-logitech-assets.sh)"
}

main() {
    cd "$(dirname "$0")"
    parse_args "$@"
    check_requirements
    plan
    sudo_once
    build
    install_user_parts
    install_system_parts
    fetch_assets
    verify
    echo
    bold "Done. Open \"Orbit\" from the app grid; it also starts with the computer, in the tray."
}

main "$@"
