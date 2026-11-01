# Architecture

## The problem in one picture

```
                 ┌───────────────────────────────── Rokid Max ───────────────────────────────┐
                 │  USB-C: DisplayPort alt-mode (1920×1080 @120 Hz)  +  USB 2.0 (IMU, 1 kHz) │
                 └───────────────▲──────────────────────────────────────────┬────────────────┘
                                 │ frames                                   │ accel/gyro/mag
┌────────────────────────────────┴──────────────────────────────────────────▼───────────────────┐
│ host (perchd on Linux/Win/macOS, or Perch Android app)                                        │
│                                                                                               │
│   perch-hal ─► perch-tracking ─► Pose (quat, ω, t)                                            │
│                      │                                                                        │
│   perch-capture ─────┼─────────► perch-compositor ─► glasses panel                            │
│   (windows/displays) │               ▲ quads from perch-space::Projector(pose, optics)        │
│                      │                                                                        │
│   perch-space (Workspace: windows, anchors, focus, presets, layouts)                          │
│                      │                                                                        │
│   perch-input Router ┴─► local sink (uinput / SendInput / CGEvent)                            │
│                        └─► remote peer (Message::Input over WebSocket)                        │
│                                                                                               │
│   perch-net: mDNS advertise, WebSocket control :7420, UDP stream :7421                        │
└───────────────────────────────────────▲───────────────────────────────────────────────────────┘
                                        │ JSON commands/events, postcard input/poses
                       ┌────────────────┴───────────────┐
                       │ controllers: Android, iPad, CLI │
                       └────────────────────────────────┘
```

## Data flow per frame (host)

1. **Device thread** (`perch-daemon/src/device.rs`) blocks on
   `Glasses::next_event()`; each `ImuSample` goes through `Tracker::feed`
   (~2 µs) and the resulting `Pose` is published on a `watch` channel.
2. **Render thread** (`render.rs`, winit main thread) wakes on v-sync, asks
   `Tracker::pose_predicted(extra)` — extrapolating by display delay (13 ms on
   Max) + configurable margin — so the image lands where the head *will* be.
3. `Engine::windows_for_render(dt)` advances smooth-follow windows, then
   `Projector::project_all` turns each window into a `ScreenQuad` (4 corners in
   panel px) sorted back-to-front.
4. Compositor uploads any new captured frames and draws each quad with the
   projective shader. In SBS modes the scene is drawn twice with ±IPD/2
   parallax into the two panel halves.
5. **Control task** (`server.rs`) handles commands from controllers, input
   batches from UDP, device events, and broadcasts `Event`s; poses go out at
   30 Hz on the control channel and at full rate on UDP to subscribers.

## Coordinate conventions (do not change)

- **World/head frame:** RUB — +X right, +Y up, +Z backward (towards wearer).
  Matches Android sensors and `ar-drivers`.
- **Pose:** unit quaternion head→world. Gravity in head frame when upright is
  `+Y·g` (accelerometer measures reaction force).
- **Yaw** = rotation about world +Y, **positive = turning left**; **pitch**
  positive = looking up. Computed from the forward vector (`-Z`), not from an
  Euler decomposition, so it's robust at any roll.
- **Placement:** `(yaw, pitch, distance, roll, width_m, height_m)` — a flat
  rectangle at `distance` facing the head origin. Angular size =
  `2·atan(w/2d)`.
- **Panel:** pixels, origin top-left, y down. `focal_px = (W/2)/tan(fov_h/2)`.

## Anchors

| Anchor | Behaviour | Use |
|---|---|---|
| `world` | Fixed direction; head motion moves it on the panel | desk work, multi-window |
| `head` | Fixed on the panel; ignores tracking | games, video, zero latency |
| `smooth_follow` | Stays until gaze leaves a 20° dead zone, then eases to centre (τ = 0.35 s) | walking, couch |

## Focus & input

Exactly one window is focused. Keyboard goes to its peer. The pointer lives in
the focused window in normalised coords; `Router` turns relative motion into
`MouseMoveAbs` for the receiver (so DPI differences don't matter) and detects
edge pushes. Past the resistance threshold, `neighbour()` finds the nearest
window in that direction *on the current projection* and focus hops.
`FocusNext`, look-to-focus (gaze dwell, optional) and the phone chips are the
other ways to move focus.

## Peers

Every process has a `Hello` (id, name, platform, capabilities). A window's
`Source` names its owning peer. When a peer disconnects its windows are hidden
(not removed) so the layout survives a phone reboot.

## Persistence

`~/.config/perch/perchd.toml` (config), `workspace.json` (windows + layouts),
`calibration-<serial>.json` (gyro bias per glasses). Android stores the same
JSON in app files.

## Threads & locking

- IMU path: device thread → `watch::Sender<Pose>`; tracker behind a
  `std::sync::Mutex` held only inside `feed`/`pose_predicted`.
- Engine behind a `tokio::sync::Mutex`; the render thread uses
  `blocking_lock` for a few microseconds per frame.
- No allocation per IMU sample after warm-up.

## Why these libraries

`nalgebra` (quaternions), `wgpu` (one renderer for Vulkan/Metal/DX12/GLES),
`winit` (fullscreen on a specific monitor), `tokio-tungstenite` (WebSocket),
`mdns-sd` (pure-Rust mDNS), `postcard` (compact, no_std-friendly wire format
that is trivial to hand-encode on phones), `ar-drivers` (open Rokid driver).
