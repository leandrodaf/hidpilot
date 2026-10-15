# Logitech Flow interop — feasibility study

Static analysis of the macOS Logi Options+ agent (`logioptionsplus_agent`,
`kiros` build, Sep 2026), carved as an x86_64 Mach-O (not stripped: ~320k
symbols). No binary was executed; no hardware was written. Sources are class
names, log format strings and decompiled getters.

## Architecture

Two layers, both built on the GPLv3 **thekogans** libraries
(`util`/`stream`/`crypto`/`packet`, github.com/thekogans):

- `logitech::marconi` — the transport (discovery, connections, encrypted framing).
- `logitech::flow` — the Flow feature on top (screen arrangement, edge crossing,
  host switch, clipboard, keyboard link).

Build paths in the binary: `logi/marconi/lib/marconi/src/*.cpp`,
`logi/provider/flow/lib/flow/src/*.cpp`.

## Discovery (marconi)

- `BroadcastDiscovery` sends/receives `BeaconPacket` over a **UDP socket**.
- `DiscoveryMgr` listens on a **TCP port** ("Starting DiscoveryMgr on TCP port: %u");
  the beacon advertises a "ping port".
- Beacon log: `Received BeaconPacket: (id, serial, channel), hostId: %s,
  from %s:%u to %s:%u, force, ping port %u, isEnabled, peer hostId`.
- `NodeStoreClient`/`NodeStoreDiscovery` is an alternate peer-address source that
  calls a Logitech cloud endpoint (`web::http`, `*.logitech.io`). LAN beacon
  discovery works without it.
- Handshake packets: `ClientHelloPacket` / `ServerHelloPacket`, then
  `ClientDeviceConnectPacket` / `ServerDeviceConnectPacket`,
  `PromoteConnectionPacket`, `HeartbeatPacket`, `PingPacket`. Hosts identify by a
  string `hostId`.

## Encrypted framing (thekogans::packet, open source)

Wire frame = `FrameHeader { crypto::ID keyId (32 bytes) ; ui32 ciphertextLength }`
followed by the ciphertext (`FrameParser.cpp`). Each ciphertext decrypts to:
`PlaintextHeader { randomLength, flags }` + random padding + optional
`Session::Header` (id + monotonic sequence number, replay-protected) + the packet
body, optionally deflated. All of this is public GPLv3 code we can reuse directly.

## Key derivation — the crux

`marconi::Device::CreateCipher()` builds the cipher as:

- `SymmetricKey::FromSecretAndSalt(secret, salt, ..., EVP_sha256, keyLen,
  ID, ...)` → **AES-256-GCM** (`EVP_aes_256_gcm`, `EVP_sha256`), key length from
  `GetCipherKeyLength`.
- The two inputs (a "random" buffer and a "password"/secret buffer) plus the
  device id and serial are the KDF material. Log string: `Master device key for:
  %u:%u`. The keyId that selects the cipher is `ID(secret-bytes)` — hence
  `DeviceMgr::GetDeviceForKeyId`.

`flow::GetDeviceChannelRandomAndPassword(id, serial, &channel, &random, &password)`
shows where those come from — **from the mouse/keyboard itself over HID++**:

- `channel` ← HID++ feature **0x1814 (ChangeHost)**, getCurrentHost.
- the `password`/secret ← HID++ feature **0x0021 (CryptoIdentifier)**,
  `GetIdentifier()` (and `RegenerateIdentifier()` to roll it). This is the device
  "crypto identifier" blob.
- `id`/`serial` ← HID++ feature **0x0003 (FirmwareInfo)**: `GetModelId()` and the
  unit/serial from `GetEntityCount`/entity 0.

So the shared secret that encrypts all Flow traffic is **read out of the shared
peripheral** by every host it is paired to. Two PCs paired to the same mouse can
each ask the mouse (feature 0x0021) for the same identifier, derive the same
AES-256-GCM key, and talk to each other. There is **no cloud secret required** for
the LAN data path — the cloud (`NodeStore`, accounts/region) is only an optional
peer-address directory and analytics.

## Flow messages (once a session is up)

`flow::` packet classes: `InitiateFlowPacket`, `SetCurrentHostPacket`,
`SetOwnerPacket`, `SettingsPacket`, `GlobalDataPacket`/`GlobalInfo`, `Device`
`DeviceDataPacket`, `keyboard::KeyPacket`, and `clipboard::*`
(`ClipboardUpdatePacket`, `FormatDataPacket`, `FileDataPacket`,
`FileDescriptorsPacket`, `GetFolderFileDescriptorsPacket` — text, images and
files). Edge geometry: `protocol.flow.EdgeHit{Edge,Point,Range,Rectangle}` and
`Config.Arrangement`/`HostConfig` (the screen layout, keyed by `keyboard_id`).
The switch itself is the same HID++ 0x1814 set-host we already drive in the CLI
(`hidpilot host`).

## Verdict

**Interop with a stock Options+ peer is feasible on the LAN, but heavy, and
brittle.** What is in reach, in order of effort:

1. **Own-agent Flow (recommended, independent of this):** a hidpilot agent on each
   PwC that does edge detection + `hidpilot host`/0x1814 switching + an encrypted
   TCP/UDP channel of our own design for clipboard. No RE needed; fully under our
   control. This is the robust path and it is what the user also asked for.
2. **Speak marconi to a real Options+ peer:** doable in principle. The framing and
   crypto are open-source GPLv3 (thekogans), the KDF is standard OpenSSL
   (HKDF-ish `FromSecretAndSalt` + AES-256-GCM), and the secret is obtainable by
   us from the shared device (HID++ 0x0021 `GetIdentifier` — Solaar does not yet
   document 0x0021; we would RE the exact request/response bytes from the devio
   `Feature0021CryptoIdentifier` getter). Remaining work is non-trivial:
   reproduce the exact `*Hello`/`*DeviceConnect`/`Promote` state machine, the
   `Session` header rules, the beacon payload layout, and the `EdgeHit`/
   arrangement protobufs. Each Options+ update can change these.

**Blockers / cautions:**
- GPLv3: thekogans is GPLv3. Reusing its code forces hidpilot's networking layer
  to be GPLv3-compatible. A clean-room reimplementation avoids that but is more work.
- HID++ 0x0021 writes: `RegenerateIdentifier()` rolls the device secret and would
  desync any other paired host — never call it during RE.
- The cloud `NodeStore`/account/region paths are the only parts that need Logitech
  servers, and only for WAN peer discovery; the LAN data path does not.

**Recommendation:** ship path (1) as the real feature. Treat path (2) as a later
"connect to an Options+ machine" compatibility mode, starting from RE of HID++
0x0021 on our own devices (read-only) and a clean-room marconi built on the public
thekogans sources.
