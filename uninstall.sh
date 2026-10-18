#!/usr/bin/env bash
# Removes what install.sh put in place. Your settings and Flow pairings in
# ~/.config/hidpilot are kept unless you say otherwise.
set -euo pipefail

BIN="$HOME/.local/bin"
UNITS="$HOME/.config/systemd/user"

ask() {
    local reply
    read -r -p "$1 [y/N] " reply
    [[ "$reply" =~ ^[YySs]$ ]]
}

for unit in hidpilot-flow hidpilot-remap hidpilot-notify; do
    systemctl --user disable --now "$unit.service" 2>/dev/null || true
    rm -f "$UNITS/$unit.service"
done
systemctl --user daemon-reload

rm -f "$BIN/hidpilot" "$BIN/hidpilot-flow" "$BIN/hidpilot-gui"
rm -f "$HOME/.local/share/applications/hidpilot.desktop"
rm -f "$HOME/.local/share/icons/hicolor/scalable/apps/hidpilot.svg"

if [[ -e /etc/systemd/system/hidpilot-battery.service || -e /etc/udev/rules.d/70-hidpilot.rules ]] &&
    ask "Remove the system parts (battery service, udev rules)? Needs sudo."; then
    sudo systemctl disable --now hidpilot-battery.service 2>/dev/null || true
    sudo rm -f /etc/systemd/system/hidpilot-battery.service /etc/udev/rules.d/70-hidpilot.rules /usr/local/bin/hidpilot \
        /etc/modules-load.d/hidpilot.conf
    sudo systemctl daemon-reload
    sudo udevadm control --reload
fi

if command -v ufw >/dev/null && ask "Remove the firewall rules added for Flow? Needs sudo."; then
    # Delete by rule text, newest first so numbering does not shift under us.
    sudo ufw status numbered | grep 'hidpilot flow' | sed -E 's/^\[ *([0-9]+)\].*/\1/' | sort -rn |
        while read -r n; do sudo ufw --force delete "$n"; done
fi

if [[ -d "$HOME/.config/hidpilot" ]] && ask "Also delete your settings and Flow pairings (~/.config/hidpilot)?"; then
    rm -rf "$HOME/.config/hidpilot"
fi

echo "hidpilot removed."
