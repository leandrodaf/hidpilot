# hidpilot

Configure Logitech peripherals on Linux, with no vendor app. Written in Rust, talks straight to `/dev/hidraw`.

**Status:** MX mice and keyboards work over HID++ 2.0 (Bolt/Unifying receivers and Bluetooth). The Zone Vibe Wireless headset works over Centurion (battery, equalizer, sidetone, mute, voice prompts, auto power off). Flow moves the mouse and keyboard between computers.

Tested on: MX Master 3S, MX Mechanical Mini (Logi Bolt receiver), Zone Vibe Wireless (Zone receiver).

## Install

```sh
./install.sh
```

It builds everything in release mode and installs, for your user:

| What | Where |
|---|---|
| `hidpilot`, `hidpilot-flow`, `hidpilot-gui` | `~/.local/bin` |
| Low battery notifications, Flow, button actions (user services) | `~/.config/systemd/user` |
| App launcher and icon | `~/.local/share/applications`, `~/.local/share/icons` |

Then it asks before each system change (all need `sudo`):

- **udev rules** (`/etc/udev/rules.d/70-hidpilot.rules`): give the logged-in user access to Logitech hidraw nodes and to `/dev/uinput`, so nothing else needs `sudo`; and tag the mirrored headset so GNOME shows it with a headset icon.
- **Battery service** (`hidpilot-battery.service`, runs as root because only root may open `/dev/uhid`): publishes mouse, keyboard and headset batteries to the kernel, so they show up in GNOME's power menu and Settings like any Bluetooth device. It is sandboxed (no network, read-only system, no capabilities, only `/dev/uhid` and hidraw). It refreshes every 5 minutes and right after the devices report a change.
- **Firewall** (only if `ufw` is installed): opens the Flow ports to your local subnet only.

Run it again after pulling changes; every step is idempotent. `./install.sh -y` answers yes to everything. `./uninstall.sh` removes it all and asks before deleting your settings and Flow pairings.

Building the desktop app needs `libwebkit2gtk-4.1-dev` and `libgtk-3-dev`; without them the rest still installs.

### Services

```sh
systemctl status hidpilot-battery            # batteries in GNOME (system)
systemctl --user status hidpilot-notify      # low battery warnings at 15%
systemctl --user status hidpilot-flow        # Flow
systemctl --user status hidpilot-remap       # custom button actions
journalctl --user -u hidpilot-flow -f        # follow a service's log
```

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
hidpilot headset --eq 4,2,0,-2,3               # five bands in dB, or --eq flat
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

## Credits

Protocol knowledge comes from Logitech's public HID++ 2.0 documents and from studying [Solaar](https://github.com/pwr-Solaar/Solaar) and [HeadsetControl](https://github.com/Sapd/HeadsetControl). No code was copied from them.

Not affiliated with Logitech. Logitech, MX and Zone are trademarks of Logitech.
