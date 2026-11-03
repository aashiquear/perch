# Platforms

One Rust core; per-platform shells. "Host" = drives the glasses. "Controller"
= touchpad/keyboard/window manager over the network. A machine can be both.

| Platform | Host | Controller | Screen source | Input sink | Notes |
|---|---|---|---|---|---|
| Linux (Ubuntu 26.04) | ✅ `perchd` | ✅ `perch` CLI | PipeWire portal ✅ (`linux-pipewire`) | uinput ✅ | primary target |
| Android (S24 Ultra) | ✅ app host mode | ✅ app touchpad | MediaProjection mirror ✅, app-per-display (see below) | Accessibility + IME ✅ | DeX caveats below |
| Windows 11 | planned (M4) | — | Windows.Graphics.Capture | SendInput | ar-drivers' rusb works on Windows with WinUSB (Zadig) |
| macOS | planned (M4) | — | ScreenCaptureKit | CGEvent | needs Screen Recording + Accessibility permissions |
| iPadOS / iOS | ❌ (no USB IMU access) | ✅ app | — | — | best trackpad of the bunch; Pencil hover moves pointer |

## Linux

**Display:** the glasses are a monitor; `perchd --features render` opens a
borderless fullscreen winit window on the monitor whose name contains
`display.monitor_name` ("Rokid"). On GNOME Wayland, keep the glasses as a
*separate* output (not mirrored) so the laptop screen stays usable.

**Build deps:** `sudo apt install libudev-dev libpipewire-0.3-dev libclang-dev`
(PipeWire headers + libclang for the `pipewire` crate's bindgen). Build with
`--features full-linux`.

**Capture:** xdg-desktop-portal ScreenCast → PipeWire. `perch pick --multiple`
opens the portal dialog; every chosen window/monitor becomes a spatial window
placed where you are looking (first at gaze, the rest fanned to the right).
The restore token is saved to `[capture] restore_token` so the next start
re-attaches the same streams without a dialog. Frames are negotiated as
BGRx/BGRA/RGBx/RGBA in CPU memory (latest frame wins); DMA-BUF is TODO. Works on GNOME,
KDE, wlroots and X11. Window-level capture gives one PipeWire stream per OS
window → one spatial window each. Virtual displays (for "extend"):
GNOME Mutter `org.gnome.Mutter.ScreenCast.RecordVirtual`, wlroots
`wlr-virtual-output`, fallback `evdi`.

**GNOME native (default when `org.gnome.Mutter.RemoteDesktop` is on the bus):**
`perch-capture::gnome` creates one virtual monitor per spatial window
(`org.gnome.Mutter.ScreenCast.Session.RecordVirtual`, size negotiated through
the PipeWire format) linked to a `org.gnome.Mutter.RemoteDesktop` session for
input (`NotifyPointerMotionAbsolute(stream, x, y)` in stream pixels, keycodes,
buttons, discrete axes). No dialogs, no `/dev/uinput`. See docs/UX.md
"Glasses displays on GNOME". The portal path below remains the fallback.

**Portal fallback:** the capture session is an xdg **RemoteDesktop**
portal session carrying the screen-cast sources, so pointer and keyboard events
are sent through the portal in *stream coordinates*: a window capture gets exact
pointer placement even though the portal never says where the window is.
`CaptureBackend::inject` returns false where the RemoteDesktop portal is
missing, and the daemon falls back to uinput below. Restore tokens for such
sessions live in the portal's `remote-desktop` table, separate from
screen-cast-only tokens.

**Input (fallback / remote peers):** `/dev/uinput` for injection (virtual "Perch Virtual Input"
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

### A. Phone as host (Perch owns the glasses) — working; see docs/ANDROID.md
`GlassesService` opens the Rokid USB interface (permission prompt once;
auto-launch via `device_filter.xml`), runs the tracker on a dedicated thread,
and shows a `Presentation` on the external display with the GL renderer.

The phone screen is the **panel** (`ui/PanelScreen.kt`): the Android twin
of the Linux dashboard. "Add app" opens any launcher app on its own
glasses display; each display is a *trusted* virtual display created by
`shell/PerchShellService.kt`, which Shizuku runs as uid shell (the scrcpy
`--new-display` mechanism), so stock Android's "only your own activities on
a virtual display" rule does not apply. Portrait ⇄ landscape per window
(`VirtualDisplay.resize`), a pop-up touchpad moves a cursor drawn by the
renderer and injects touch/scroll into that display, Back/Home/Recents keys
likewise. Without Shizuku: the MediaProjection mirror window only.

**DeX conflict:** when you plug the glasses in, Samsung may start DeX on
them. Turn off "Auto start DeX when HDMI/DP connected"; DeX draws directly
to the display and Perch cannot composite it (Samsung limitation).

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
