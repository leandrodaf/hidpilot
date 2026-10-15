# Zone Vibe Wireless protocol notes

Receiver `046d:0af0` (headset paired) / `046d:0af1` (receiver alone).

| Interface | What |
|---|---|
| 0 | HID: consumer keys (report 1), telephony (5, 9), vendor pages |
| 1 | DFU firmware updater. **Never write here.** |
| 2-4 | USB audio |

## Transport

The headset speaks Centurion (the HID++ successor used by newer Logitech headsets),
addressed variant, on report `0x22` of interface 0 (usage page `0xFF53`):

```
22 | addr | cpl_len | flags | feat_idx | fn<<4 | swid | params...   (padded to 64 bytes)
```

- `addr` = `0xF1`. Other addresses: `0xFF`/`0x01` are refused with EPIPE; `0x00` timed
  out and made the kernel reset the receiver (the headset dropped off for a few seconds).
- `cpl_len` counts flags + feature index + function byte + params.
- Errors come back as `feat_idx = 0xFF`, then the original index, function, error code.

The headset answers directly at `0xF1`; there is no CentPPBridge (0x0003).

## Features (firmware reports 12)

| Idx | ID | Name | Known functions |
|---|---|---|---|
| 0 | 0x0000 | Root | fn0 getFeature, fn1 protocol version (1.32) |
| 1 | 0x0001 | FeatureSet | fn0 count, fn1 [remaining, id_hi, id_lo, type, version] |
| 2 | 0x0100 | DeviceInfo | fn2 serial |
| 3 | 0x0101 | DeviceName | fn0 [max_len, len, "Zone Vibe Wireless"] |
| 4 | 0x0104 | BatterySOC | fn0 [percent, percent, charging] |
| 5 | 0x0108 | AutoSleep | fn0 get minutes, fn1 set minutes (default 30) |
| 6 | 0x0109 | ? | fn1 → 0x0a, fn3 → 0x01 |
| 7 | 0x0201 | HeadsetEQ | fn0 get (format unknown), fn1 is a setter |
| 8 | 0x0303 | ? | |
| 9 | 0x0500 | ? | |
| 10 | 0x0501 | ? | |
| 11 | 0x0502 | ? | |

Probing rule learned the hard way: odd functions are often setters and the frame is
zero padded, so calling them "to see" writes zeros. Only call functions known to be getters.
