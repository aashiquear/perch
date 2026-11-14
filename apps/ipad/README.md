# Perch for iPad

Two tabs, as in the Android app:

- **Glasses** — the iPad drives the glasses itself. Plug them into the iPad's
  USB-C port and your windows hang in space around you. Made for travel: a
  seat, the glasses, headphones, no laptop.
- **Laptop remote** — the iPad is the panel, trackpad, keyboard and game
  controller for a computer running `perchd`.

Status (14 Nov 2026): **working on an iPad with M2, iPadOS 27.0.1**, Rokid
Max, Bose Noise Cancelling Headphones 700. YouTube, Xbox Cloud Gaming,
GeForce NOW, a self-hosted web app over Tailscale and sign-in with saved
passwords were all used on the device. What was measured is in
`.claude/memory/hardware-log.md`.

## What iPadOS allows, and what Perch does about it

| The Android host does | iPadOS | So on the iPad |
|---|---|---|
| reads the glasses' motion sensor over USB | refuses apps any USB access to it (every route tried returns "not permitted") | head motion comes from **Bose AR headphones** over Bluetooth LE, 49 samples a second |
| shows any installed app in a window | no app may display another app | windows are **web pages and files** the app renders: sites, web apps, video, PDF, pictures |
| injects touches into those apps | no public way to touch a window on a display that takes no touches | taps are delivered as **genuine touches** through UIKit internals (a switch turns this off) |
| draws at 120 Hz | offers the glasses at 60 Hz | 1920×1080 at 60 Hz |

Without headphones the app still works: windows simply stay in front of your
eyes. The tracker is coarser than the glasses' own 1 kHz sensor; seated, with
prediction, it reads as steady.

## Glasses tab

**Start.** Plug in the glasses (the app must be on screen: if you switch to
another app, iPadOS shows the mirrored iPad instead). Switch the headphones
on, tap **Head tracking**, look straight ahead with a level head and tap
**Recenter**.

**Windows.** Tap a bookmark, or **Add window** for an address, a search or a
file from the iPad (films and documents downloaded before a flight). The
focused window's card carries the **address bar**: type an address or words
to search. IP addresses, `host:port`, `localhost` and `.local` names open as
`http://`, so a server on your network or tailnet needs no scheme
(`100.64.0.5:8080`). ☆ keeps the page on the **bookmark bar**; hold a
bookmark to open it in a new window or remove it. Open pages come back next
time.

**Sign-in.** *on iPad* brings the live page onto the iPad screen: real
touches, the keyboard, and the system's password AutoFill (iCloud Keychain,
or Chrome or another manager chosen under Settings → Passwords). Send it back
to the glasses and you stay signed in, in every window and after a restart.
No app can borrow another browser's sessions, and passkeys are not offered
to a web view; use a password and code.

**Pop-up** (Touchpad or Gamepad):

- Touchpad: one finger moves the cursor in the focused window, tap = tap,
  two fingers scroll, two-finger tap = page back, three-finger swipe = next
  window; corner locks for drag mode and spring-back scrollers. A keyboard,
  trackpad or mouse attached to the iPad works too; the keyboard button
  opens a typing field.
- Xbox, Wheel and Flight skins, and any controller paired with the iPad,
  reach the page as a standard gamepad (cloud gaming).
- Under the pad: **back** (leaves fullscreen, else goes back a page), next
  window, pin here, recenter, bigger / smaller, nearer / further, and **move
  window**: while it is on, the arrow pad swaps the focused window with its
  neighbour.

**Games and video.** *fill view* makes a window the whole panel, pixel for
pixel, following the head. A page's own fullscreen button fills its window.
Sites that normally insist on "Add to Home Screen" on an iPad (Xbox Cloud,
GeForce NOW) are told they were launched that way.

**Also on the panel.** Arrange (side by side, centred), Close all, a
brightness slider for the glasses picture, the system volume and a sound
output picker (with glasses attached iPadOS may send sound to the glasses'
speakers, whose level it cannot change: pick the headphones), the *Genuine
taps* switch, and **Check**, which repeats the device measurements and can
send a diagnostic report to the laptop (`nc -l -p 7430`).

**Known limits.** No drag in pages. Protected streaming may refuse an
embedded browser. Mouse-look (pointer lock) is not supported. Pages are kept
in the window: a link that tries to open another app is blocked, because
leaving Perch drops the glasses back to mirroring.

## Laptop remote tab

Open the app on the same Wi-Fi as a `perchd` host, tap the host (or type its
IP; add the pairing token if it has one). The panel mirrors the host: glasses
and calibration state, layout map, To glasses (full / half / wide / tall),
Recenter, Calibrate, Arrange and presets, Display mode, brightness, window
cards (focus, pin, width, close). The pop-up is the same touchpad, keyboard
and controller, sent to the laptop; ⌘ is sent as Ctrl by default.

## Install

**From a release.** Download `perch-vX.Y.Z-ipad-unsigned.ipa`. It is
unsigned on purpose: an iPad only runs apps signed for its owner's Apple
account, so the tool that installs it signs it with yours.

```bash
xtool install perch-vX.Y.Z-ipad-unsigned.ipa     # Linux, Windows or macOS; https://xtool.sh
```

AltStore and Sideloadly do the same. With a free Apple ID the signature lasts
7 days; install again to renew (your data stays). The first time, trust the
developer on the iPad (Settings → General → VPN & Device Management), and
allow Local Network and Bluetooth when the app asks.

**From source, on Linux — no Mac.** One-time: Swift (swiftly), `xtool setup`
(asks for an Xcode `.xip` and your Apple ID), `rustup target add
aarch64-apple-ios`. Then, iPad on USB and unlocked:

```bash
just ipad          # Rust core for iOS, build, sign, install
just ipad-test     # Swift core tests + page-script tests
just ipad-ipa      # the unsigned IPA a release carries, without installing
```

On a Mac the same commands work; `xtool dev generate-xcode-project` gives an
Xcode project.

## How it is built

```
Package.swift          SwiftPM; xtool.yml, Info.plist, AppIcon.png
Sources/PerchCore      plain Foundation, tested on the build machine:
                       Postcard (wire bytes), TouchpadGestures, RemoteInput,
                       HostState, BoseAR (headset bytes), WebAddress, Bookmarks,
                       StandardGamepad
Sources/CPerch         the C header of crates/perch-ffi (symlink)
Sources/Perch          the app:
  Host/GlassesHost     frame loop: Rust pose → homography → CATransform3D
  Host/HostWindow      one window = one live WKWebView
  Host/HostScripts     what is injected into pages (taps, fullscreen, gamepad)
  Host/TouchSynth      genuine taps
  Host/BoseProbe       the headphones (CoreBluetooth)
  Host/ControllerBridge  paired controllers (GameController)
  Host/HostLab, UsbProbe  the on-device check
  Net/PerchClient      perchd client (Bonjour, WebSocket, UDP)
  UI/*, Touchpad/*     panel, pop-up, controller skins
Tests/PerchCoreTests   38 tests: wire bytes, gestures, input, state, addresses
Tests/PageScripts      run.sh: the injected scripts in headless Chrome;
                       wk.py: a live site in WebKitGTK with the same scripts
```

The maths is the shared Rust core, as on Android: `perch-tracking`'s
`external` module turns headset orientations into head poses (frame
conversion, mounting tilt removed at recentre, prediction on the sensor's
clock) and `perch-space` projects the windows; both reach Swift through
`perch-ffi`, linked as a static library. A window is not captured or
re-encoded: the homography goes straight onto the web view's layer.

`Postcard.swift` is pinned to `wire_format_tests` in `crates/perch-proto`;
change the three together (CLAUDE.md, rule 3).
