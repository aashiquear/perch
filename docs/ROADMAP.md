# Roadmap

Milestones are sized for evenings-and-weekends with Claude Code. Each has a
definition of done you can verify on your own hardware.

## M0 — Scaffold (this release, v0.1.0) ✅
- Rust workspace, pure crates with tests, daemon/CLI runnable with `--mock`.
- Android + iPad apps written against the real protocol.
- Docs, Claude memory, CI.

## M1 — Tracking on real glasses (Linux + Android)
**Done when:** glasses plugged into the Bosgame → `perch status` shows the Max,
`perch watch` streams poses (IMU at 444 Hz), yaw drift < 1°/min after
`perch calibrate`, and the Android host app does the same on the S24 Ultra.
- [x] `just hw-check` on Ubuntu (2026-10-31); see `.claude/memory/hardware-log.md`.
  Measured: **444 Hz** IMU (not 1 kHz — "~1 kHz" above is wrong for Max), yaw drift
  0.08 °/min after calibration (target met), EDID has 1920×1200@120.
- [ ] Drop the stale startup burst (≈9 packets with boot-clock timestamps and
  ±40 rad/s gyro) in `perch-hal` before they reach the tracker.
- [ ] `perch mode mono120` doesn't change the host's refresh; add a host-side
  mode switch (GNOME `gdctl`/Mutter D-Bus, wlr-output-management) or document it.
- [ ] Render-on-glasses: match monitors by EDID description on Wayland (winit
  only gives "DP-5"); default `display.monitor_name = "Rokid"` never matches.
- [ ] udev: `70-perch-uinput.rules` should grant `uaccess` (or document `input` group).
- [ ] Confirm axis signs (left turn → +yaw). Add `tracker.invert_axes` if needed.
- [ ] Measure FOV (procedure in DEVICE-ROKID-MAX.md) and set the default.
- [ ] Temple button events on Max — do they arrive? If not, drop the mapping.
- [x] Android: USB permission + fd path + IMU thread (444 Hz on the S24 Ultra, 2026-11-01); multi-app, touchpad, keyboard, gamepad verified (2026-11-02).
- [x] Online gyro-bias learning while still (`auto_bias`, 2026-11-02); `use_mag` stays off.
- [ ] Tune `kp/ki`.

## M2 — Real pixels (Linux capture) 
**Done when:** `coding` preset shows three *actual* windows from the laptop
(e.g. editor, terminal, browser) pinned in space, 120 Hz, no tearing.
- [x] `perch-capture` Linux: portal ScreenCast → PipeWire, per-window streams,
      CPU (MemFd/MemPtr) frames → wgpu. `perch pick [--multiple]` opens the
      portal dialog; picked windows land where you look. (2026-11-01)
- [ ] DMA-BUF import into wgpu (zero copy); offer modifiers in the format pod.
- [x] Persist the portal restore token so the prompt appears once
      (`[capture] restore_token` in perchd.toml; stale tokens are cleared).
- [ ] Verify on hardware: 3 real windows, 120 Hz, no tearing; measure
      capture→photon latency.
- [ ] Virtual displays on GNOME (`RecordVirtual`) so windows can be moved
      *into the glasses* like extra monitors; wlroots path.
- [ ] Window picker in `perch` CLI and phone app (`enumerate()` → chips).
- [ ] Focus outline + cursor rendering in the compositor.
- [ ] Android: scrcpy/ADB `Source` so the phone/DeX appears as a window on the laptop host.

## M3 — Seamless input
- [ ] **Pointer escapes onto the glasses output.** Mutter requires outputs to be
      edge-adjacent (corner-only layouts are rejected: "Logical monitors not
      adjacent"), so the OS pointer can always wander onto DP-5. Interim
      (2026-11-01): perchd hides the OS cursor and draws its own on the glasses
      so it can be found and brought back. Real fix options: (a) perchd owns
      the mouse via evdev grab + uinput and keeps the OS pointer on the laptop;
      (b) GNOME shell extension pointer barrier on the shared edge; (c) treat
      pointer-in-glasses as "point at a spatial window" and route clicks to
      the window's source (needs per-window pointer mapping).
**Done when:** one keyboard+mouse on the laptop drives the laptop windows *and*
the phone window; pushing the pointer off the right edge of the editor moves
into the browser pinned to its right; typing on the phone touchpad lands in
whatever is focused.
- [x] Linux evdev source thread in `perchd` (`share_local_devices`), grab on
      demand (bar / panel / Scroll Lock), spatial pointer on the glasses,
      absolute pointer injection for monitor captures via a uinput tablet
      device + GNOME DisplayConfig layout (2026-11-01; needs the `input` group).
- [ ] Verify the uinput absolute device maps to the whole desktop on Mutter;
      per-window screen rects (needs a GNOME extension or Mutter window
      introspection) so clicks land in window captures too.
- [ ] Grab the pointer *only* while it is over a Perch window (edge barrier)
      so the laptop stays usable with the glasses on.
- [ ] Android Accessibility/IME injection polish; cursor overlay on phone.
- [ ] Look-to-focus (gaze dwell) behind a setting.
- [ ] Stereo (SBS) mode end-to-end with IPD setting; verify parallax comfort.
- [ ] Exclusive/passthrough mode for fullscreen games on the glasses output.

## M4 — Windows & macOS hosts, phone fidelity
- [ ] Windows: WinUSB via Zadig doc, WGC capture, SendInput/hook input, installer.
- [ ] macOS: ScreenCaptureKit, CGEvent, notarised app bundle.
- [ ] Android: Shizuku `INJECT_EVENTS` backend; DeX app-per-window via
      `setLaunchDisplayId`; settings screen (FOV, IPD, sensitivity).
- [ ] iPad: keyboard shortcut forwarding (⌘ → ctrl mapping), layout map view.

## M5 — Polish & trust
- [ ] Transport security (Noise/TLS + QR pairing), replacing the `#token` hack.
- [ ] Auto-start (`systemd --user` unit, Android boot receiver).
- [ ] Settings UI on all platforms; `perch doctor`.
- [ ] Latency measurement using the glasses' VSync timestamps.
- [ ] Packages: `.deb`, Flatpak (portal-friendly), APK on GitHub Releases, TestFlight.

## Stretch
- Curved mega-screen preset; window snapping/grid; voice "pin here".
- Other glasses via `ar-drivers` (XREAL Air, Viture) — HAL is already generic.
- OpenXR/Monado bridge so Perch windows show inside XR apps.

## Known risks
- Android app-per-window needs privileged permissions on stock firmware.
- PipeWire DMA-BUF modifiers vs wgpu import: may need `ash` interop code.
- Rokid firmware updates could change report layout; `ar-drivers` pins to
  what was observed in 2023–24. Keep the recording tool handy.
