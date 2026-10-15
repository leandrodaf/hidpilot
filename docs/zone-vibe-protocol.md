# Zone Vibe Wireless protocol notes

Receiver `046d:0af0` (headset paired) / `046d:0af1` (receiver alone).

| Interface | What |
|---|---|
| 0 | HID: consumer keys (report 1), telephony (5, 9), vendor pages, Centurion (report 0x22) |
| 1 | DFU firmware updater. **Never write here.** |
| 2-4 | USB audio |

Sources: live reads on the device (getters only) and a static study of Logi Tune 3.x for
Windows (`LogiTuneAgent.exe`, C++ namespace `centurionpp`, and the Electron UI). Logi Tune
calls the headset family "Enduro" (Zone Vibe 100/130/Wireless share it).

## Transport

Centurion (the HID++ successor used by newer Logitech headsets), addressed variant, on
report `0x22` of interface 0 (usage page `0xFF53`), as output/input reports of 64 bytes:

```
22 | addr | cpl_len | flags | feat_idx | fn<<4 | swid | params...   (zero padded to 64 bytes)
```

- `addr` = `0xF1`. `0xFF`/`0x01` are refused with EPIPE; `0x00` timed out and made the
  kernel reset the receiver (the headset dropped off for a few seconds).
- `cpl_len` counts flags + feature index + function byte + params. **The device reads the
  params from the padding when `cpl_len` is short**: a setter sent with no params is applied
  with whatever the firmware finds (that is how the EQ and the name got corrupted, see below).
- `flags` = fragment info, 0 for single-frame messages.
- Replies echo `feat_idx` and `fn<<4|swid`. Errors: `feat_idx = 0xFF`, then the original
  index, function byte and an error code (`02` = not supported by this device, `05` =
  invalid argument).
- Events (notifications) arrive with `swid = 0`; the function nibble is the event number.
- Logi Tune uses its own software id; use a different one per client (this repo's probes use
  `0xD`) so replies are not mixed up.

The headset answers directly at `0xF1`; there is no CentPPBridge (0x0003) on this receiver.

## Feature table

The firmware reports 12 features. Function numbers below come from Logi Tune's
`Feature<id>i<Name>` classes; "get"/"set" was confirmed from how the agent builds and parses
each call.

| Idx | ID | Logi Tune class | Notes |
|---|---|---|---|
| 0 | 0x0000 | Root | fn0 getFeature(id_hi, id_lo) → [idx, type, version]; fn1 ping/protocol version (1.32) |
| 1 | 0x0001 | FeatureSet | fn0 count; fn1(i) → [remaining, id_hi, id_lo, type, version] |
| 2 | 0x0100 | DeviceInfo | fn0 hardware info → [model 0x22, hw rev 1, product id (u16)]; fn1(entity) firmware → [type, ?, ver_hi, ver_lo, name_len, name]; fn2 serial → [len, ascii]. fn4 is used by the factory-reset path: **do not call** |
| 3 | 0x0101 | DeviceName | see below |
| 4 | 0x0104 | BatterySOC | fn0 → [percent, percent, charging]; event 0 same payload |
| 5 | 0x0108 | AutoSleep | fn0 get, fn1 set; see below |
| 6 | 0x0109 | Earcon | voice-prompt language; see below |
| 7 | 0x0201 | EQSet | 5-band EQ; see below |
| 8 | 0x0303 | — | not used by Logi Tune (no class in the agent). fn1 → `04`. Unknown |
| 9 | 0x0500 | HeadsetAudio | mute, sidetone (ANC/SBC functions exist but return error 02 here) |
| 10 | 0x0501 | HeadsetBtConnInfo | Bluetooth connections and pairing history |
| 11 | 0x0502 | HeadsetMisc | voice notifications, rotate-to-mute, buttons, factory reset |

### 0x0101 DeviceName (idx 3)

| Fn | Kind | Params | Reply |
|---|---|---|---|
| 0 | get | — | `[max_len, len, name...]` (max_len = 30) |
| 1 | set | `[len, name bytes..., 0x00]` (the agent appends the NUL) | — |

Event 0: `[len, name...]` when the name changes. There is no "restore default" function.
Logi Tune exposes this as `/devices/<id>/name` (rename) and `/devices/<id>/name/maxlen`.

### 0x0104 BatterySOC (idx 4)

fn0 get → `[soc %, soc %, charging]` (0 discharging, 1/2 charging, 3 full). Event 0 same.

### 0x0108 AutoSleep (idx 5) — `/devices/<id>/sleep_timer`

- fn0 get. Version 1 replies one byte: minutes until power off (factory 30 = `1e`). Newer
  versions reply `[shutdown, deep, long, mul_shutdown, mul_deep, mul_long]` with each value
  multiplied by its multiplier.
- fn1 set `[minutes]`. The agent sends 60 when asked for 256 or more.

### 0x0109 Earcon (idx 6) — voice prompt language

| Fn | Kind | Notes |
|---|---|---|
| 0 | set | `[language_id]` (select an installed language) |
| 1 | get | current language id (this unit: `0a`) |
| 3 | ? | replies `01`; not used by Logi Tune for this device |
| 4 | get | installed languages, 32-bit bitmask; byte 3 holds ids 0-7, byte 2 ids 8-15... (this unit: `00 00 04 00` = only id 10) |

Event 0: `[language_id]` after a change. New language packs are flashed through the DFU
features of the receiver (not present on this unit's list).

### 0x0201 EQSet (idx 7) — `/audio/<id>/equalizer`

Payload of get (fn0), set (fn1) and event 0 is 6 bytes:

```
[mode, band1, band2, band3, band4, band5]
```

- `band1..5`: signed int8 gains in whole dB. Logi Tune's centurion equalizer declares
  margins -128..127 with integers only; the UI uses a 5-band slider and four built-in
  presets (Default, Volume boost, Podcast, Bass boost) plus up to 8 custom presets kept on the
  PC, not in the headset.
- `mode`: passed through untouched by Logi Tune (stored next to the gains). The factory
  value read on this unit was `00`.
- Factory state on this unit: `00 00 00 00 00 00` (flat).

### 0x0500 HeadsetAudio (idx 9) — `/devices/<id>/audio/...`

| Fn | Kind | Setting | On this unit |
|---|---|---|---|
| 0 | get | mic mute (bool) | `01` (muted, boom up) |
| 1 | set | mic mute `[bool]` | |
| 2 | get | sidetone level: low nibble = value, high nibble set = negative | `05` |
| 3 | set | sidetone level `[level]`; Logi Tune sends `[0xFF]` right after to persist it on some models | |
| 4 / 5 | get / set | ANC | error 02 (no ANC on the Vibe) |
| 6 | set | sidetone on/off `[bool]` | |
| 7 / 8 | get / set | SBC bitpool `[enabled, a, b]` | error 02 |
| 11 / 12 | get / set | ANC button customization | error 02 |

Events: 0 mute `[bool]`, 1 ANC, 3 ANC customization, 4 sidetone level `[level]`.

The UI shows sidetone as 0-100 % in steps of 10 and passes the number straight to the agent;
the device range looks like 0-10 (same as Zone Wired in HeadsetControl, and this unit reads 5).
Not verified with a write.

### 0x0501 HeadsetBtConnInfo (idx 10) — connection history

| Fn | Kind | Notes |
|---|---|---|
| 0 | get | number of connected Bluetooth hosts (this unit: 1) |
| 2 | get | device name of a host |
| 3 | get | connection status of history entry `[index]` |
| 4 | get | active audio device |
| 7 | get | connected device info / A2DP mute |
| 8 | set | A2DP mute of a host |
| 9 | get | number of entries in the pairing history (this unit: 8) |
| 11 | set | remove a host from the history |

Events: device status change and audio status change.

### 0x0502 HeadsetMisc (idx 11)

| Fn | Kind | Setting | On this unit |
|---|---|---|---|
| 0 / 1 | get / set | Do Not Disturb | error 02 |
| 2 / 3 | get / set | voice notifications `[bool]` (`/misc/voice_notification`) | `01` |
| 4 | action | **factory reset** — never call while probing | |
| 5 / 6 | get / set | rotate (flip) to mute `[bool]` (`/misc/rotate_to_mute`) | `01` |
| 9 | get | button customization | |
| 10 | get | button capability | |
| 11 | set | button customization | |
| 12 | action | reset button customization | |

Events: 0 voice notification change `[bool]`, 2 mic boom position change `[state]`.

## Restoring values damaged while probing

Both were caused by calling a setter with no params (see the transport note on padding).

```python
from cent_lib import Cent          # scratch helper: Cent().call(idx, fn, params)
c = Cent()                         # /dev/hidraw7 = interface 0 of 046d:0af0, addr 0xF1
c.call(7, 1, [0, 0, 0, 0, 0, 0])   # EQ back to flat (factory 000000000000; now d80f00000000)
c.call(3, 1, [0x12] + list(b"Zone Vibe Wireless") + [0x00])   # name back to the factory one
```

Frames on the wire:

```
22 f1 09 00 07 1d 00 00 00 00 00 00                       (EQ)
22 f1 17 00 03 1d 12 5a 6f 6e 65 20 56 69 62 65 20 57 69 72 65 6c 65 73 73 00   (name)
```

## Probing rules

- Odd function numbers are often setters and frames are zero padded, so calling one "to see"
  writes zeros. Call only functions listed above as getters.
- 0x0100 fn4 and 0x0502 fn4 lead to factory reset in Logi Tune.

## Open questions

- 0x0303 (idx 8): not used by Logi Tune, purpose unknown.
- 0x0109 fn3 meaning; mapping of language ids to languages (10 is the only one installed).
- Exact device range of the sidetone level (assumed 0-10).
- Meaning of the first EQ byte (`mode`).
