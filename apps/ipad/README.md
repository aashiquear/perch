# Perch for iPad / iPhone

The iPad twin of the Android app's panel, pointed at a host over the network:
the same dashboard (header, stat tiles, layout map, actions two per row,
presets, window cards) and the same pop-up (touchpad, keyboard, arrow pad,
Xbox / wheel / flight controller skins).

Two tabs, as on Android: **Laptop remote** (below) and **Glasses**.

## Glasses tab — the iPad as host

Plug the glasses into the iPad's USB-C port; the app draws on them at
1920×1080, 60 Hz. What iPadOS allows shapes the rest (all measured on the
device, `.claude/memory/hardware-log.md`, 2026-11-14):

- **Head tracking comes from Bose AR headphones**, not the glasses: iPadOS
  refuses apps the glasses' USB sensor. A Noise Cancelling Headphones 700
  gives its rotation over Bluetooth LE at 49 Hz. Tap **Head tracking**, look
  straight ahead with a level head, tap **Recenter**. Without a tracker the
  windows simply stay in front of the eyes.
- **Windows are things the app renders**: web pages, and files from the iPad
  (video, PDF, pictures — what you download before a flight). iPadOS does not
  let an app show other apps.
- **Address bar and bookmarks.** The focused window's card has the address
  bar (address or search; IPs, `host:port`, `localhost` and `.local` open as
  `http://`, so a server on the LAN or a tailnet needs no scheme). ☆ keeps
  the page on the bookmark bar; hold a bookmark to open it in a new window
  or remove it. Open pages are restored next time.
- **Sign-in: "on iPad".** Brings the live page onto the iPad screen with real
  touches, the keyboard and password AutoFill (iCloud Keychain, or Chrome /
  another manager chosen in Settings → Passwords), then back to the glasses.
  One sign-in holds for every window and launch. No app can reuse Chrome's or
  Safari's own sessions, and passkeys are not offered to a web view.
- **Games.** "fill view" makes the window the whole panel, pixel for pixel,
  following the head. **Gamepad** opens the Xbox / wheel / flight skins; they
  and any controller paired with the iPad reach the page as a standard
  gamepad. Fullscreen buttons on pages fill the window.
- **Layout.** Nearer / Further move the whole row; pins survive adding and
  closing windows until Arrange; the slider dims the glasses picture.
- **Add window** → address, search, bookmark, or file. **Touchpad** opens the
  pop-up: the cursor is drawn in the focused window, tap = click, two fingers
  scroll, two-finger tap = page back, three-finger swipe = next window; the
  keyboard button types into the page. Cards: pin where I look, tall / wide,
  bigger / smaller, back / forward / reload, close.
- **Check** repeats the device measurements (display, USB sensor, headphones).

Code: `Host/GlassesHost.swift` (frame loop: Rust pose → homography →
`CATransform3D` on each `WKWebView`), `Host/HostWindow.swift`,
`Host/BoseProbe.swift`; maths in `crates/perch-tracking/src/external.rs`
and `perch-space` through `perch-ffi`.

## What it does

| On the panel | Effect on the host |
|---|---|
| To glasses (full / half / wide / tall) | moves the laptop's focused window onto a glasses display |
| Touchpad, Gamepad | open the pop-up |
| Recenter, Calibrate, Display, brightness slider | the glasses, through the host |
| Arrange | re-flow, presets (single, coding, gaming, couch), saved layouts |
| All back, ✕ on a card | windows return to the laptop screen |
| Window card | tap = focus, pin where I look; focused: width (narrow / normal / wide / auto), layout nearer / further, close app |

Pop-up: 1 finger moves the pointer, tap = click, 2-finger tap = right click,
double-tap-and-hold or a stroke from the rim = drag, 2 fingers = scroll,
3-finger swipe = next window; corner locks for drag mode and the spring-back
scrollers. A keyboard, trackpad or mouse attached to the iPad drives the
laptop while the pop-up is open; ⌘ is sent as Ctrl by default (switch on the
panel). The keyboard button opens a typing field for the on-screen keyboard.

## Build and install (Linux, no Mac)

Uses [xtool](https://xtool.sh): SwiftPM + the iOS SDK from an Xcode `.xip`.
One-time: install Swift (swiftly), run `xtool setup` (asks for the `.xip` and
your Apple ID). Then, with the iPad on USB and unlocked:

```bash
cd apps/ipad
# the shared Rust core, once and after Rust changes
cargo rustc -p perch-ffi --release --target aarch64-apple-ios --crate-type staticlib
xtool dev build --ipa     # → xtool/Perch.ipa
xtool install xtool/Perch.ipa
```

`just ipad` and `just ipad-test` do the same from the repo root. With a free
Apple ID the app is signed for 7 days; run `xtool install` again to renew.
The first time, trust the developer on the iPad (Settings → General → VPN &
Device Management) and allow Local Network access when the app asks.

On a Mac: `xtool dev generate-xcode-project`, or the same `xtool dev`.

## Layout

```
Package.swift        two targets + tests; xtool.yml, Info.plist, AppIcon.png
Sources/PerchCore    plain Foundation, no UIKit: Postcard (wire bytes),
                     TouchpadGestures, RemoteInput (keys, controller state),
                     HostState (the host's `state` event), BoseAR (headset bytes)
Sources/CPerch       the C header of crates/perch-ffi (symlink)
Sources/Perch        SwiftUI shell: Net/PerchClient, UI/*, Touchpad/*, Host/*
Tests/PerchCoreTests byte-exact wire tests + gesture / input / state tests
```

Logic that can be tested lives in `PerchCore` and runs on the build machine:

```bash
PERCH_CORE_ONLY=1 swift test      # the variable leaves the SwiftUI target out
```

`Postcard.swift` is pinned to `wire_format_tests` in `crates/perch-proto`;
change the three together (CLAUDE.md, rule 3).

## Use

Start `perchd` on the laptop, open Perch on the iPad on the same Wi-Fi, tap
the host (or type its IP; add the pairing token if the host has one). Focus a
window on the laptop and tap **To glasses**; open **Touchpad** to drive it.
