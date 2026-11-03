# Wire protocol v1.0

Source of truth: `crates/perch-proto/src/lib.rs`. This page is the human
summary for people writing clients in Kotlin/Swift/JS.

## Transports

| Channel | Transport | Encoding | Port | Carries |
|---|---|---|---|---|
| control | WebSocket, text frames | JSON, `Message` | 7420 | hello, commands, events, state, routed input to a peer |
| stream | UDP datagrams | postcard, `StreamPacket` | 7421 | poses (host→subscribers), input batches (controller→host), ping/pong |

Discovery: mDNS/DNS-SD `_perch._tcp.local.`, TXT `peer_id=<uuid>`, `proto=1.0`.

## Handshake

Client sends `Message::Hello` first; server replies with its own `Hello`.
Major versions must match. If the host has a pairing token, the client's
`name` must end with `#<token>` (v0.1 scheme; LAN only).

```json
{"msg":"hello","version":{"major":1,"minor":0},"peer_id":"…","name":"S24 Ultra",
 "platform":"android","caps":{"glasses_host":false,"screen_source":false,
 "input_sink":true,"input_source":true,"dex":true}}
```

## Control messages (internally tagged on `msg`)

- `{"msg":"hello", …}`
- `{"msg":"command","cmd":"<name>", …fields}` — controller → host
- `{"msg":"event","event":"<name>", …fields}` — host → controllers
- `{"msg":"input","events":[…]}` — host → the peer that owns the focused
  window (that peer injects them locally)
- `{"msg":"pose", …}` — low-rate pose (30 Hz) for UIs
- `{"msg":"ping","nonce":n}` / `{"msg":"pong","nonce":n}`

### Commands

| `cmd` | fields | effect |
|---|---|---|
| `recenter` | — | current gaze → yaw 0 |
| `calibrate` | `seconds` | stationary gyro-bias calibration |
| `set_display_mode` | `mode`: `mono`/`stereo`/`mono120`/`stereo120` | USB control transfer to glasses |
| `set_brightness` | `percent` | if supported |
| `upsert_window` | `window` | add/replace |
| `remove_window` | `id` | |
| `place_window` | `id`, `placement` | move/resize |
| `pin_here` | `id` | place at current gaze, anchor → world |
| `set_anchor` | `id`, `anchor`: `world`/`head`/`smooth_follow` | |
| `focus` | `id` | |
| `focus_next` | — | cycle visible windows |
| `save_layout` / `load_layout` | `name` | |
| `apply_preset` | `name`: `single`/`coding`/`gaming`/`couch` | |
| `get_state` | — | host replies with `state` |
| `set_grid` | `grid` | since 1.2 (shape changed in 1.3): replace the layout grid (`rows`, `row_h_m`, `gap_m`, `distance`, `center_yaw`, `center_pitch`, `px_per_m`, `max_w_m`); every row is re-flowed, `window_changed` per moved window, then `state` |
| `slot_window` | `id`, `row`, `index` | since 1.2: put a window into `row` at `index` (0 = leftmost, past the end appends); the row re-flows, `window_changed` for every window that moved |
| `arrange_grid` | — | since 1.2: re-flow every visible window into the rows in z order, then `state` |
| `set_window_span` | `id`, `span` | since 1.4: width in layout blocks (`grid.block_w_m`), 0 = automatic from pixels |
| `move_to_glasses` | `width`, `height` | since 1.5: move the host's focused window onto a new glasses display of that size (GNOME: a virtual monitor); falls back to `pick_sources`. `remove_window` moves it back |
| `close_app_window` | `id` | since 1.5: close the app behind the window (platform close shortcut), then remove it |
| `resize_display` | `id`, `width`, `height` | since 1.5: re-create the glasses display at a new size and move the app onto it |
| `set_window_pixels` | `id`, `width`, `height` | since 1.3: source pixel size of a window; sets its width in the grid (`px_per_m`) |
| `pick_sources` | `multiple` | since 1.1: host opens its source picker (Linux: desktop portal); each chosen window/monitor becomes a window near the gaze (`window_changed` ×n, then `focus_changed`), or `error` if cancelled |

### Events

`glasses_connected{model,serial}`, `glasses_disconnected`,
`calibration{…CalibrationState}`, `pose{…}`, `state{…State}`,
`window_changed{…SpatialWindow}`, `window_removed{id}`,
`focus_changed{id|null}`, `error{message}`.

### Types

```json
"placement": {"yaw":0.3,"pitch":-0.1,"distance":2.0,"roll":0.0,"width_m":1.6,"height_m":0.9}
"source":    {"kind":"display","peer":"…","display_index":0}
             {"kind":"window","peer":"…","handle":"0x3a00005","title":"nvim"}
             {"kind":"virtual","peer":"…","width":1920,"height":1080}
             {"kind":"dex","peer":"…"}   {"kind":"phone_screen","peer":"…"}
"window":    {"id":"…","source":{…},"placement":{…},"anchor":"world","z":0,
              "opacity":1.0,"visible":true,"focused":false}
"pose":      {"t_us":123,"orientation":[w,x,y,z],"angular_velocity":[x,y,z],"confidence":1.0}
"calibration": "uncalibrated" | {"calibrating":{"percent":42}} | "ready"
```

Angles are radians; the CLI converts degrees for humans.

## Stream packets (postcard, externally tagged)

```
StreamPacket = varint(variant) ++ payload
  0 Pose   : t_us u64 varint, [w,x,y,z] f32×4 LE, [x,y,z] f32×3 LE, confidence f32
  1 Input  : varint(len) ++ InputEvent×len
  2 Ping   : u32 varint
  3 Pong   : u32 varint

InputEvent = varint(variant) ++ fields
  0 mouse_move      dx f32, dy f32          (logical px of target)
  1 mouse_move_abs  x f32, y f32            (0..1 of target window)
  2 mouse_button    button varint(0 L,1 R,2 M,3 Back,4 Fwd), down u8
  3 scroll          dx f32, dy f32          (lines; + = up/left)
  4 key             usage varint (HID page 7), down u8, mods u8 (1 ctrl,2 shift,4 alt,8 gui)
  5 text            UTF-8 string: varint(len) ++ bytes
  6 gamepad         axes f32×8 LE (lx, ly, rx, ry −1..1 with y down; lt, rt 0..1; hat x, y −1/0/1),
                    buttons varint u16 (bit 0 A, 1 B, 2 C, 3 X, 4 Y, 5 Z, 6 L1, 7 R1, 8 L2, 9 R2,
                    10 select, 11 start, 12 mode, 13 L3, 14 R3)   — since 1.6
```

Byte-exact examples (also in `wire_format_tests`, `PostcardTest.kt`,
`PostcardTests.swift`):

```
Ping(1)    → 02 01
Ping(300)  → 02 AC 02
Input[MouseMove(1,-2), MouseButton(Right,down), Scroll(0,3), Key(0x04,up,mods=3), Text("é"), Gamepad(lx=0.5, rt=1, hat_y=-1, buttons=0x0101)]
           → 01 06
             00 00 00 80 3F 00 00 00 C0
             02 01 01
             03 00 00 00 00 00 00 40 40
             04 04 00 03
             05 02 C3 A9
             06 00 00 00 3F 00×20 00 00 80 3F 00 00 00 00 00 00 80 BF 81 02
```

A controller subscribes to poses by sending any `Ping` on UDP; the host keeps
sending poses to that address for 10 s after the last ping.

## Versioning

Current: **1.6** (`gamepad` input event, presented by the host as a virtual Xbox-class controller; 1.5 = `move_to_glasses`, `close_app_window`, `resize_display`; 1.4 = `set_window_span`, `grid.block_w_m`; 1.3 = flow-row grid: `Grid` fields and `slot_window` arguments changed, `set_window_pixels` added; 1.2 added the grid commands and `state.grid`; 1.1 added `pick_sources`). 
`PROTOCOL_VERSION.major` bumps on incompatible change. Add fields with
`#[serde(default)]` to stay minor-compatible.
