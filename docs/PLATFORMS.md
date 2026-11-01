# Platforms

One Rust core; per-platform shells. "Host" = drives the glasses. "Controller"
= touchpad/keyboard/window manager over the network. A machine can be both.

| Platform | Host | Controller | Screen source | Input sink | Notes |
|---|---|---|---|---|---|
| Linux (Ubuntu 26.04) | ✅ `perchd` | ✅ `perch` CLI | PipeWire portal (M2) | uinput ✅ | primary target |
| Android (S24 Ultra) | ✅ app host mode | ✅ app touchpad | MediaProjection mirror ✅, app-per-display (see below) | Accessibility + IME ✅ | DeX caveats below |
| Windows 11 | planned (M4) | — | Windows.Graphics.Capture | SendInput | ar-drivers' rusb works on Windows with WinUSB (Zadig) |
| macOS | planned (M4) | — | ScreenCaptureKit | CGEvent | needs Screen Recording + Accessibility permissions |
| iPadOS / iOS | ❌ (no USB IMU access) | ✅ app | — | — | best trackpad of the bunch; Pencil hover moves pointer |

## Linux

**Display:** the glasses are a monitor; `perchd --features render` opens a
borderless fullscreen winit window on the monitor whose name contains
`display.monitor_name` ("Rokid"). On GNOME Wayland, keep the glasses as a
*separate* output (not mirrored) so the laptop screen stays usable.

**Capture (M2):** xdg-desktop-portal ScreenCast → PipeWire. Works on GNOME,
KDE, wlroots and X11. Window-level capture gives one PipeWire stream per OS
window → one spatial window each. Virtual displays (for "extend"):
GNOME Mutter `org.gnome.Mutter.ScreenCast.RecordVirtual`, wlroots
`wlr-virtual-output`, fallback `evdi`.

**Input:** `/dev/uinput` for injection (virtual "Perch Virtual Input"
keyboard+mouse), evdev grab for capture when routing to a remote peer. User in
`input` group + `udev/70-perch-uinput.rules`. Panic key (default Scroll Lock)
always returns input to local.

**Gaming:** use `anchor=head` (zero tracking latency) or `world` with the
gaming preset; the compositor's Mailbox present mode avoids extra frames of
latency. Steam games in a Perch window go through capture, which adds a frame;
for competitive play run the game fullscreen on the glasses output directly
and use Perch only for side windows (M3: passthrough/exclusive mode).

## Android (Samsung S24 Ultra)

Two very different ways to use the phone:

### A. Phone as host (Perch owns the glasses)
`GlassesService` opens the Rokid USB interface (permission prompt once;
auto-launch via `device_filter.xml`), runs the tracker at 1 kHz, and shows a
`Presentation` on the external display with the GL renderer.

Windows you can add today:
- **Mirror of the phone screen** (MediaProjection) — the "phone as floating
  window" case. Works everywhere.
- **Own-app displays** (`AppDisplaySource`): a VirtualDisplay you launch
  activities into. Stock Android only allows *your own* activities on a
  non-trusted virtual display.

To get **arbitrary apps as separate windows** (true multi-window), you need
one of:
1. **DeX "Force desktop mode"** (Developer options) + Shizuku to grant
   `INTERNAL_SYSTEM_WINDOW`; then `ActivityOptions.setLaunchDisplayId` works
   for other apps. Recommended on Samsung.
2. `adb shell settings put global force_desktop_mode_on_external_displays 1`
   and `enable_freeform_support 1` — the external display becomes a freeform
   desktop; Perch then captures it as one window (not per-app).
3. Root.

**DeX conflict:** when you plug the glasses in, Samsung may start DeX on
them. Either turn off "Auto start DeX when HDMI/DP connected", or use mode B
below and let DeX be the desktop (no 3DOF on Samsung's compositor; that is a
Samsung limitation — DeX draws directly to the display).

### B. Phone as DeX desktop + laptop as host
Run DeX on the glasses *through the laptop*: Samsung DeX for PC is
discontinued, so the practical path is **scrcpy** (`scrcpy --display-id=<dex>`
or the phone screen) captured by `perchd` as a window source (M2 adds a
scrcpy/ADB `Source`). Then the phone is just another peer, input is routed to
it via `adb shell input` or the Perch Accessibility service.

### C. Phone as controller
`TouchpadScreen`: trackpad, remote keyboard, window chips (tap = focus,
long-press = pin), presets. mDNS finds `perchd`.

**Injecting into the phone** when a phone window is focused: Accessibility
`dispatchGesture` for taps/drags/scroll; the Perch IME for text and non-
printing keys. Not perfect (no hover, no modifier combos in arbitrary apps);
for full fidelity use Shizuku `INJECT_EVENTS` (M4).

Build: `cargo install cargo-ndk && rustup target add aarch64-linux-android`,
then `./gradlew assembleDebug` (runs `cargo ndk` for `perch-ffi --features rokid`).

## iPadOS / iOS

Controller only. `NWBrowser` for Bonjour, `URLSessionWebSocketTask` for
control, `NWConnection` UDP for input. Attach a keyboard to the iPad and it
forwards typing; the Pencil's hover moves the pointer. Build with XcodeGen
(`apps/ipad/project.yml`).

## Windows / macOS (M4)

`perchd` already compiles for both minus the backends. Windows: Zadig/WinUSB
on the Rokid sensor interface for `rusb`; capture with WGC; input with
`SendInput` + low-level hooks. macOS: `rusb` works out of the box; capture
ScreenCaptureKit; input `CGEventPost`/`CGEventTap` (Accessibility permission).
