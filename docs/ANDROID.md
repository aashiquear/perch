# Perch on Android — phone as host

The phone drives the Rokid Max alone: USB-C carries DisplayPort video to the
glasses and the IMU back; the Perch app runs the shared Rust tracker and
spatial core (`perch-ffi`) and renders the 3DOF windows on the glasses'
display with GL. The phone's own screen is the **panel**: the same dashboard
as the Linux control panel — pick apps, see the layout, move things, a
pop-up touchpad.

Status (Nov 2026): designed and implemented against the APIs below; **not
yet verified on the S24 Ultra** (that is what the debug APK is for). Facts
that are device-specific are marked *to verify*.

## What an app can and cannot do without root

Rendering other apps into windows of our own needs each app to run on a
display we draw from. Android's rules (AOSP activity-launch policy; see
`WindowSources.kt` and the links at the end):

| mechanism | third-party apps? | needs |
|---|---|---|
| MediaProjection mirror of the phone screen | one window = the whole phone screen | user prompt |
| own `VirtualDisplay`, `setLaunchDisplayId` | only our own activities | — |
| `VIRTUAL_DISPLAY_FLAG_TRUSTED` virtual display | **any app**, one per display | `ADD_TRUSTED_DISPLAY` → shell uid or system |
| Samsung DeX on the external display | DeX desktop, not per-window | Samsung |

So real multi-app windows need **shell privileges**, which is exactly what
scrcpy's `--new-display` uses and what [Shizuku](https://shizuku.rikka.app)
hands to apps: the user starts Shizuku once per boot (Wireless debugging, no
PC after the first time), and Perch binds a *user service* that runs as
`uid 2000 (shell)`. In that process Perch can:

- `DisplayManager.createVirtualDisplay(name, w, h, dpi, surface, flags)` with
  `TRUSTED | OWN_DISPLAY_GROUP | ALWAYS_UNLOCKED | OWN_FOCUS` — a display
  that any app may be launched on, that keeps its own focus and stays
  unlocked when the phone locks;
- `am start --display <id> -n <pkg>/<activity>` to put an app there;
- `InputManager.injectInputEvent` with the event's display id set — touch
  and mouse into that display, without the Accessibility service.

Without Shizuku the app still works with the mirror window (phone screen as
one floating window) and the laptop-touchpad mode.

## Architecture

```
phone screen (panel, Compose)             glasses display (Presentation + GL)
┌───────────────────────────┐             ┌──────────────────────────────────┐
│ Rokid Max · tracking 444Hz│             │   [Chrome]      [Signal]  …      │
│ ▣ To glasses  ▣ Touchpad  │   pose      │        ▲ cursor in focused one   │
│ ▣ Recenter    ▣ Calibrate │ ─────────►  │  SpatialRenderer: one external   │
│ windows: Chrome ⟳ ✕       │             │  texture per window (SurfaceTex) │
│          Signal ⟳ ✕       │             └──────────────────────────────────┘
└───────────────────────────┘                        ▲ Surface
          │ AIDL (Binder)                             │
          ▼                                           │
┌────────────────────────────────────────────────────┴──┐
│ PerchShell (Shizuku user service, uid shell)           │
│  createDisplay(w,h,dpi,surface) → trusted VirtualDisplay│
│  launch(displayId, package)      → am start --display   │
│  inject(displayId, MotionEvent)  → InputManager         │
│  resize(displayId, w, h)        → VirtualDisplay.resize │
└────────────────────────────────────────────────────────┘
```

- **One window = one virtual display.** The renderer creates the
  `SurfaceTexture`, hands its `Surface` across Binder; the display renders
  straight into it (no copy, no encode). Window size on the glasses comes
  from the display's pixel aspect, like Linux.
- **Portrait / landscape per window.** The display is created portrait
  (1080×2340, phone-like) or landscape (2340×1080) and can be flipped with
  `VirtualDisplay.resize` — the app gets a configuration change, the phone
  screen itself never rotates.
- **Cursor and touchpad.** A pop-up sheet on the panel (same gestures as
  the laptop touchpad: 1 finger move, tap = tap, 2 fingers scroll, 3-finger
  swipe = next/previous window) moves a cursor drawn by the renderer in the
  focused window; taps become touch events injected at the cursor, scrolls
  become mouse-wheel events, both routed to that window's display id.
  Keyboard: a Bluetooth keyboard paired with the phone types into the focused
  display's app once it has focus (`OWN_FOCUS`); the panel's text field
  injects key events as a fallback. *To verify: key routing to a trusted
  display with OWN_FOCUS on One UI.*
- **Focus** = the window last tapped on the panel's list, or 3-finger swipe.
- **Recenter**: temple button (IMU key event) or panel button. Calibration
  saved to app storage, same JSON as Linux.
- **Quit**: releasing a display destroys the app's task on it (Android
  finishes activities when their display goes away).

## Panel (Compose) — mirrors the Linux dashboard

Header: glasses picture when a Rokid is attached, model, IMU Hz, calibration,
mode chip (TOUCHPAD when the sheet is up). Stat tiles: windows, fps, Shizuku
state. Square icon buttons with text beside: **Add app** (opens the app
picker: launcher apps with icons, search), **Touchpad**, **Recenter**,
**Calibrate**, **Arrange** (presets: 1×1, 3 equal, wide centre, 2×2),
**All back** (release every display). Window cards: app icon + name, focus
dot, ⟳ portrait/landscape, ✕ close (release the display), "nearer/farther".
The laptop-touchpad mode (phone as controller for `perchd`) stays as a
second tab.

## Build and test

```bash
rustup target add aarch64-linux-android && cargo install cargo-ndk
cd apps/android && gradle assembleDebug       # or ./gradlew; builds perch-ffi first
adb install -r app/build/outputs/apk/debug/app-debug.apk
```

On the phone: install Shizuku, start it (Wireless debugging), open Perch →
allow Shizuku → plug the glasses → **Start** → **Add app**.

Things to verify on the device (not yet done):
1. `RokidAir::new(fd)` over `UsbManager` fd and the IMU rate on the S24.
2. Trusted virtual display creation from the Shizuku service (flags
   `0x400|0x800|0x1000|0x4000`), and `am start --display`.
3. Injected touch reaching the app on that display; keyboard routing.
4. DeX auto-start when the glasses are plugged in: turn off "Auto start DeX
   when HDMI/DP is connected", or Samsung draws DeX on the glasses instead.

Sources: AOSP activity launch policy
(https://source.android.com/docs/core/display/multi_display/activity-launch),
scrcpy virtual display (`--new-display`, server `VirtualDisplayListener`),
Shizuku user-service docs.
