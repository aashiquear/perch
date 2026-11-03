# Perch on Android — phone as host

The phone drives the Rokid Max alone: USB-C carries DisplayPort video to the
glasses and the IMU back; the Perch app runs the shared Rust tracker and
spatial core (`perch-ffi`) and renders the 3DOF windows on the glasses'
display with GL. The phone's own screen is the **panel**: the same dashboard
as the Linux control panel — pick apps, see the layout, move things, a
pop-up touchpad.

Status (3 Nov 2026): **working on a Samsung S24 Ultra (Android 16, One UI
8.5)**: 3DOF tracking, multi-app windows, touchpad, keyboard, Bluetooth
input and the virtual gamepad (verified in Chrome's gamepad tester, GeForce
NOW and Xbox Cloud Gaming). The "Findings" section at the end is the record
of what was measured on the device.

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
  Keyboard: a Bluetooth keyboard paired with the phone is forwarded to the
  focused window as key events while Perch is on screen; a mouse uses
  pointer capture so it never clicks the panel.
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

All four of the original open questions are settled (see Findings): the USB
fd path gives 444 Hz, trusted displays and `am start --display` work from the
helper, injected touch and keys reach the app, and DeX auto-start must be off.

Sources: AOSP activity launch policy
(https://source.android.com/docs/core/display/multi_display/activity-launch),
scrcpy virtual display (`--new-display`, server `VirtualDisplayListener`),
Shizuku user-service docs.

## Findings from the first on-device round (S24 Ultra, 2026-11-01)

- Trusted display creation, `am start --display` and touch injection all work
  from the Shizuku helper once it claims package `com.android.shell`.
- App displays **rotate with their content** (`FLAG_ROTATES_WITH_CONTENT`):
  YouTube's fullscreen turns its display landscape, a portrait-locked app
  turns a landscape display portrait. The buffer keeps its size with the
  picture rotated inside it; the renderer maps the texture back upright
  (`bufferUv`) and gives the window the logical shape. `wm
  set-ignore-orientation-request` is no longer used (it is persisted per
  display and leaked into the phone's own behaviour).
- Home and Recents keys act on the phone's own display, not the window, so the
  touchpad offers Back, next window, rotate, bigger/smaller, nearer/further and
  re-open instead.
- 3DOF needs the real IMU path (Start, not Start (mock)); the mock feeds a
  still synthetic IMU so windows stay put.
- Apps that set `FLAG_SECURE` (banking, DRM video) render black on every
  virtual display that is not itself secure, and a secure virtual display
  needs `CAPTURE_SECURE_VIDEO_OUTPUT`, a signature permission the shell uid
  does not hold. Perch checks the display's windows 3 s after launch
  (`isSecure`, via `dumpsys window`) and labels the card instead.
- A Bluetooth mouse uses pointer capture on the panel (`requestPointerCapture`):
  the phone's pointer disappears and relative motion drives the glasses
  cursor, so clicks never land on the panel. The panel switch releases it.
- Virtual gamepad: a UHID device ("Perch Gamepad", vendor 045e / product
  02fd, Xbox-like descriptor) created from the shell helper. Android applies
  `Vendor_045e_Product_02fd.kl`; sources KEYBOARD|GAMEPAD|JOYSTICK, all axes
  present (X/Y/Z/RZ, BRAKE→LTRIGGER, GAS→RTRIGGER, HAT). The pop-up writes
  **HID input reports** to it (13 bytes), so games get kernel-level
  controller input. Kernel events go to Android's *focused display*; the
  pop-up is therefore a separate non-focusable window in gamepad modes (so
  tapping it does not pull focus to the phone) and Perch forwards any
  controller events that still reach it. Injection with the device id
  remains as fallback. LT/RT drive the analog trigger axes as well.
- Keyboard: the soft keyboard is only shown when the phone holds an IME
  target window, so the pop-up is *focusable* in Touchpad mode (typing works
  as on a phone) and non-focusable only for gamepad modes. The keyboard
  button under the d-pad opens a typing bar whose text is converted to key
  events for the focused window, independent of whether the app asked for an
  IME (Word's canvas does not).
- Brightness: Android 16 accepts `cmd display set-brightness --id <glasses>`
  but the Rokid panel ignores the value; the slider is software dimming in
  the renderer. The 1920×1080 @120 Hz mode is requested via
  `preferredDisplayModeId`.
- Tracking: the one-off calibration leaves a slowly wandering yaw bias; the
  tracker now learns the bias online whenever the glasses are still
  (`TrackerConfig.auto_bias`), after which drift is negligible.
- Scroll: two-finger movement is a finger drag at the cursor (Touch mode) or
  wheel events (Mouse mode); the scroll-lock corner shows spring-back
  scrollers that send wheel events, which apps with their own scroll bars
  (Word) honour.
- Glasses "brightness" is software dimming in the renderer (colour × factor);
  the Rokid driver exposes no hardware brightness, the temple rocker does.
- `wm set-ignore-orientation-request` is reset on each display before it is
  released, and on the phone display when all windows go back, if it was off
  before Perch started.
