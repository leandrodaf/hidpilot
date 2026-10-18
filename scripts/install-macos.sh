#!/bin/bash
# Installs the Flow agent on a Mac: builds hidpilot-flow, starts it at login (LaunchAgent)
# and installs SwitchAudioSource so the headset's sound can follow the mouse.
# macOS needs no driver: the Logi Bolt receiver and the Zone headset are plain USB/Bluetooth
# devices there. Run from the repository: ./scripts/install-macos.sh   (--uninstall removes it)
set -euo pipefail

LABEL="io.github.leandrodaf.hidpilot.flow"
BIN="$HOME/.local/bin"
PLIST="$HOME/Library/LaunchAgents/$LABEL.plist"
LOG="$HOME/Library/Logs/hidpilot-flow.log"
APP="$HOME/Applications/Orbit Flow.app"
cd "$(dirname "$0")/.."

bold() { printf '\033[1m%s\033[0m\n' "$*"; }
fail() { printf '\033[31m%s\033[0m\n' "$*" >&2; exit 1; }

[[ "$(uname)" == Darwin ]] || fail "This script is for macOS; on Linux use ./install.sh"

if [[ "${1:-}" == "--uninstall" ]]; then
    launchctl bootout "gui/$(id -u)/$LABEL" 2>/dev/null || true
    rm -f "$PLIST" "$BIN/hidpilot-flow"
    rm -rf "$APP"
    echo "hidpilot-flow removed (pairings kept in ~/Library/Application Support/hidpilot)."
    exit 0
fi

bold "hidpilot will install on this Mac:"
echo "  • hidpilot-flow in $BIN, started at login"
echo "  • \"Orbit Flow\" in ~/Applications (Launchpad, Spotlight): opens the Flow page"
echo "  • SwitchAudioSource (Homebrew), so the headset's sound can move with the mouse"
echo

# Build tools: Xcode command line tools and Rust.
xcode-select -p >/dev/null 2>&1 || { xcode-select --install; fail "Install the Xcode command line tools (a window opened), then run this again."; }
if ! command -v cargo >/dev/null; then
    [[ -x "$HOME/.cargo/bin/cargo" ]] || curl https://sh.rustup.rs -sSf | sh -s -- -y
    export PATH="$HOME/.cargo/bin:$PATH"
fi

bold "Building (a few minutes the first time)..."
cargo build --release -p hidpilot-flow
install -d "$BIN"
install -m755 target/release/hidpilot-flow "$BIN/hidpilot-flow"

if command -v brew >/dev/null; then
    command -v SwitchAudioSource >/dev/null || brew install switchaudio-osx
else
    echo "Homebrew not found: without SwitchAudioSource the mouse and keyboard still move, the sound does not."
fi

# Creates the identity and settings on first run.
"$BIN/hidpilot-flow" peers >/dev/null

# A small app bundle so the Flow page has a place in Launchpad and Spotlight; it just
# runs "hidpilot-flow ui". The agent itself is the LaunchAgent below.
install_app() {
    rm -rf "$APP"
    install -d "$APP/Contents/MacOS" "$APP/Contents/Resources"
    cat > "$APP/Contents/MacOS/orbit-flow" <<EOS
#!/bin/bash
exec "$BIN/hidpilot-flow" ui
EOS
    chmod 755 "$APP/Contents/MacOS/orbit-flow"
    cat > "$APP/Contents/Info.plist" <<EOS
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key><string>Orbit Flow</string>
    <key>CFBundleDisplayName</key><string>Orbit Flow</string>
    <key>CFBundleIdentifier</key><string>$LABEL.ui</string>
    <key>CFBundleVersion</key><string>1.0.0</string>
    <key>CFBundleShortVersionString</key><string>1.0.0</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleExecutable</key><string>orbit-flow</string>
    <key>CFBundleIconFile</key><string>orbit</string>
    <key>LSUIElement</key><true/>
</dict>
</plist>
EOS
    # Icon from the project's PNGs; sips and iconutil ship with macOS.
    local set; set=$(mktemp -d)/orbit.iconset
    mkdir -p "$set"
    for px in 16 32 128 256 512; do
        sips -z $px $px packaging/icons/hidpilot-256.png --out "$set/icon_${px}x${px}.png" >/dev/null 2>&1 || true
    done
    iconutil -c icns "$set" -o "$APP/Contents/Resources/orbit.icns" 2>/dev/null || true
    # Spotlight and Launchpad pick the bundle up once it is registered.
    /System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister -f "$APP" >/dev/null 2>&1 || true
}
install_app

PATHS="$BIN:/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin"
cat > "$PLIST" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key><string>$LABEL</string>
    <key>ProgramArguments</key><array><string>$BIN/hidpilot-flow</string><string>run</string></array>
    <key>EnvironmentVariables</key><dict><key>PATH</key><string>$PATHS</string></dict>
    <key>RunAtLoad</key><true/>
    <key>KeepAlive</key><true/>
    <key>StandardOutPath</key><string>$LOG</string>
    <key>StandardErrorPath</key><string>$LOG</string>
</dict>
</plist>
EOF
launchctl bootout "gui/$(id -u)/$LABEL" 2>/dev/null || true
launchctl bootstrap "gui/$(id -u)" "$PLIST"
sleep 2

if launchctl print "gui/$(id -u)/$LABEL" 2>/dev/null | grep -q "state = running"; then
    bold "Done: hidpilot-flow is running and starts with your login."
else
    fail "hidpilot-flow did not start; see $LOG"
fi
cat <<EOF

The Flow page is opening in your browser: pick the Linux computer under "On this network",
click where its screen is, and type the code it shows. That's it.
(Reopen it any time: "Orbit Flow" in Launchpad or Spotlight, or: hidpilot-flow ui)

  • If macOS asks, allow incoming connections for hidpilot-flow; if the mouse does not
    switch, give it "Input Monitoring" in System Settings → Privacy & Security.
  • For the sound to come along, connect the Zone Vibe to this Mac over Bluetooth once.

Log: $LOG
EOF
"$BIN/hidpilot-flow" ui || true
