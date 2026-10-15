# hidpilot

Configure Logitech peripherals on Linux, with no vendor app. Written in Rust, talks straight to `/dev/hidraw`.

**Status:** MX mice and keyboards work over HID++ 2.0 (Bolt/Unifying receivers and Bluetooth). Support for the Zone Vibe Wireless headset is being reverse engineered.

Tested on: MX Master 3S, MX Mechanical Mini (Logi Bolt receiver).

## Install

```sh
cargo install --path .
sudo cp udev/70-hidpilot.rules /etc/udev/rules.d/
sudo udevadm control --reload && sudo udevadm trigger --subsystem-match=hidraw
```

The udev rule gives the logged-in user access to Logitech hidraw nodes, so no `sudo` is needed afterwards.

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
hidpilot host master                           # Easy-Switch hosts
hidpilot host master 2                         # switch to host 2
hidpilot buttons master                        # reprogrammable buttons
```

Devices are picked by list number or by any part of their name.

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
| Change host / hosts info | 0x1814 / 0x1815 | `host` |
| Backlight | 0x1982 | `backlight` |
| Reprogrammable controls v4 | 0x1b04 | `buttons` |
| SmartShift (and enhanced) | 0x2110 / 0x2111 | `smartshift` |
| High resolution wheel | 0x2121 | `wheel` |
| Thumb wheel | 0x2150 | `thumbwheel` |
| Adjustable DPI | 0x2201 | `dpi` |
| Fn inversion | 0x40a3 | `fn-keys` |

## Roadmap

- Zone Vibe Wireless headset: battery, sidetone, equalizer, auto power off, voice prompts
- Graphical interface

## Credits

Protocol knowledge comes from Logitech's public HID++ 2.0 documents and from studying [Solaar](https://github.com/pwr-Solaar/Solaar) and [HeadsetControl](https://github.com/Sapd/HeadsetControl). No code was copied from them.

Not affiliated with Logitech. Logitech, MX and Zone are trademarks of Logitech.
