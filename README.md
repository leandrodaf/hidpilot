# hidpilot

Configure Logitech peripherals on Linux, with no vendor app. Written in Rust, talks straight to `/dev/hidraw`.

**Status:** MX mice and keyboards work over HID++ 2.0 (Bolt/Unifying receivers and Bluetooth). The Zone Vibe Wireless headset works over Centurion (battery, equalizer, sidetone, mute, voice prompts, auto power off). Flow moves the mouse and keyboard between computers.

Tested on: MX Master 3S, MX Mechanical Mini (Logi Bolt receiver), Zone Vibe Wireless (Zone receiver).

## Install

You need Ubuntu 24.04 or similar (GNOME, X11 or Wayland for the app; Flow needs X11), and
three minutes.

**1. Install the build tools** (once):

```sh
sudo apt install build-essential pkg-config libwebkit2gtk-4.1-dev libgtk-3-dev \
                 libayatana-appindicator3-1 p7zip-full git curl
curl https://sh.rustup.rs -sSf | sh        # Rust, if you do not have it
```

**2. Get hidpilot and install it:**

```sh
git clone https://github.com/leandrodaf/hidpilot.git
cd hidpilot
./install.sh
```

The installer shows what it will do, asks once to continue and once for your password, and
then does everything: builds, installs the programs and the app, starts the services, sets up
the device rules, opens Flow in the firewall for your local network, downloads Logitech's
device pictures and fonts from Logitech's own installers, and finally checks that each part
is running and that GNOME sees your batteries.

**3. Open "hidpilot"** from the app grid. From then on it starts with the computer and lives
in the tray (closing or minimizing the window keeps it there).

Options: `./install.sh --help` (`-y` skips the question; `--no-flow`, `--no-remap`,
`--no-firewall`, `--no-assets` leave parts out). To update, `git pull && ./install.sh`.
To remove everything, `./uninstall.sh` (it asks before deleting your settings).

### Instalação (pt-BR)

```sh
sudo apt install build-essential pkg-config libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-1 p7zip-full git curl
curl https://sh.rustup.rs -sSf | sh              # Rust, se ainda não tiver
git clone https://github.com/leandrodaf/hidpilot.git && cd hidpilot
./install.sh                                     # pergunta uma vez e pede a senha uma vez
```

Depois abra **hidpilot** na grade de aplicativos. Ele inicia junto com o computador e fica na
bandeja; para atualizar, `git pull && ./install.sh`; para remover, `./uninstall.sh`.

### What gets installed

| What | Where |
|---|---|
| `hidpilot` (CLI), `hidpilot-flow`, `hidpilot-gui` (the app) | `~/.local/bin` |
| App launcher, icon, start at login | `~/.local/share/applications`, `~/.local/share/icons`, `~/.config/autostart` |
| Low battery notifications, Flow, button actions (user services) | `~/.config/systemd/user` |
| Device access rules | `/etc/udev/rules.d/70-hidpilot.rules` |
| Battery service, root, sandboxed: no network, read-only system, no capabilities | `/etc/systemd/system/hidpilot-battery.service` |
| Logitech's pictures and fonts, taken from Logitech's installers on your machine (never shipped with hidpilot) | `~/.local/share/hidpilot/logitech` |

The kernel has no driver for the Logi Bolt receiver, so without hidpilot GNOME does not know
the devices behind it. The battery service is what makes them appear in GNOME's power
settings, with their names and levels.

### Services

```sh
systemctl status hidpilot-battery            # batteries in GNOME (system)
systemctl --user status hidpilot-notify      # low battery warnings at 15%
systemctl --user status hidpilot-flow        # Flow
systemctl --user status hidpilot-remap       # custom button actions
journalctl --user -u hidpilot-flow -f        # follow a service's log
```

The app writes what it does with its window and tray to `~/.local/state/hidpilot/gui.log`.

## Usage

```sh
hidpilot list                                  # connected devices
hidpilot info master                           # firmware, battery, settings, features
hidpilot battery                               # battery of every device
hidpilot dpi master 1600                       # pointer sensitivity
hidpilot smartshift master --mode ratchet --threshold 20
hidpilot wheel master --hires on --invert off
hidpilot thumbwheel master --invert on
hidpilot backlight mechanical --mode off
hidpilot fn-keys mechanical on                 # F1-F12 without holding Fn
hidpilot platform mechanical linux             # key layout (same as Fn+O / Fn+P)
hidpilot host master                           # Easy-Switch hosts
hidpilot host master 2                         # switch to host 2
hidpilot buttons master                        # reprogrammable buttons
```

Devices are picked by list number or by any part of their name.

### Headset

```sh
hidpilot headset                               # battery, equalizer, sidetone, mic...
hidpilot headset --eq podcast                  # a preset (default, volume-boost, podcast, bass-boost)
hidpilot headset --eq 40,20,0,-20,30           # or five bands, -128..127 (0 = flat)
hidpilot headset --sidetone 5                  # hear your own voice, 0-10
hidpilot headset --mute on
hidpilot headset --voice-prompts off
hidpilot headset --rotate-to-mute on           # flip the boom up to mute
hidpilot headset --auto-sleep 30               # minutes before it powers off
hidpilot headset --name "Zone Vibe Wireless"
```

## Flow

Push the pointer against the screen edge facing another computer and the mouse and keyboard switch to it, with the clipboard. Both computers run `hidpilot-flow` (Linux, Windows and macOS builds come from CI) and the devices must be paired with both (Easy-Switch channels).

```sh
hidpilot-flow pair                             # on one computer: shows a 6-digit code
hidpilot-flow pair --to "other-pc"             # on the other: type the code
hidpilot-flow place "other-pc" right           # where its screen is
hidpilot-flow peers                            # paired computers
hidpilot-flow set --clipboard false            # keep the clipboard to yourself
```


Security: nothing is ever typed or clicked over the network; Flow only tells the other computer where the pointer enters and switches the device's Easy-Switch channel. Pairing uses SPAKE2 on the code (three wrong tries close the window) and afterwards Noise IK with pinned keys, so only paired computers can talk. It uses TCP 47811 (and 47812 while pairing) and mDNS, on the local network only.
## Button remapping and gestures

`hidpilot remap run` is a user service that gives buttons, the gesture button, the thumb
wheel and keyboard special keys new actions, like Logi Options+. Without configuration the
gesture button gets the Options+ gestures: click or swipe up for the activities overview,
down for the desktop, left/right to change workspace.

```sh
hidpilot remap buttons master                  # buttons that take an action
hidpilot remap actions                         # what they can do
hidpilot remap set master back key:alt+left
hidpilot remap set master forward tab-next --app firefox    # only while Firefox has focus
hidpilot remap set mechanical emoji cmd:gnome-characters
hidpilot remap thumbwheel master volume
hidpilot remap run
```

Settings live in `~/.config/hidpilot/buttons.toml` and are reloaded while the service runs.
Only buttons with an action in the profile of the focused app are diverted; the rest keep
their normal behaviour, and stopping the service gives every button back.

## Supported HID++ features

| Feature | ID | Commands |
|---|---|---|
| Firmware info | 0x0003 | `info` |
| Unified battery | 0x1004 | `battery`, `info` |
| Change host / hosts info | 0x1814 / 0x1815 | `host`, Flow |
| Backlight | 0x1982 | `backlight` |
| Reprogrammable controls v4 | 0x1b04 | `buttons` |
| SmartShift (and enhanced) | 0x2110 / 0x2111 | `smartshift` |
| High resolution wheel | 0x2121 | `wheel` |
| Thumb wheel | 0x2150 | `thumbwheel` |
| Adjustable DPI | 0x2201 | `dpi` |
| Fn inversion | 0x40a3 | `fn-keys` |
| Multi-platform | 0x4531 | `platform` |

The headset protocol is documented in [docs/zone-vibe-protocol.md](docs/zone-vibe-protocol.md).

## Roadmap

- Audio that follows the pointer between computers (headset multipoint or audio over the network)
- Flow with computers running Logi Options+ ([docs/flow-interop.md](docs/flow-interop.md))
- Device-specific icons in GNOME (UPower files every `/dev/uhid` battery as a generic battery)

## Credits

Protocol knowledge comes from Logitech's public HID++ 2.0 documents and from studying [Solaar](https://github.com/pwr-Solaar/Solaar) and [HeadsetControl](https://github.com/Sapd/HeadsetControl). No code was copied from them.

Not affiliated with Logitech. Logitech, MX and Zone are trademarks of Logitech.
