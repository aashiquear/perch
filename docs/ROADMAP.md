# Roadmap

Milestones are sized for evenings-and-weekends with Claude Code. Each has a
definition of done you can verify on your own hardware.

## M0 — Scaffold (this release, v0.1.0) ✅
- Rust workspace, pure crates with tests, daemon/CLI runnable with `--mock`.
- Android + iPad apps written against the real protocol.
- Docs, Claude memory, CI.

## M1 — Tracking on real glasses (Linux + Android)
**Done when:** glasses plugged into the Bosgame → `perch status` shows the Max,
`perch watch` streams poses at ~1 kHz, yaw drift < 1°/min after
`perch calibrate`, and the Android host app does the same on the S24 Ultra.
- [ ] `just hw-check`; fill `.claude/memory/hardware-log.md`.
- [ ] Confirm axis signs (left turn → +yaw). Add `tracker.invert_axes` if needed.
- [ ] Measure FOV (procedure in DEVICE-ROKID-MAX.md) and set the default.
- [ ] Temple button events on Max — do they arrive? If not, drop the mapping.
- [ ] Android: USB permission + fd path + 1 kHz thread; verify no frame drops.
- [ ] Tune `kp/ki`; try `use_mag=true` and decide.

## M2 — Real pixels (Linux capture) 
**Done when:** `coding` preset shows three *actual* windows from the laptop
(e.g. editor, terminal, browser) pinned in space, 120 Hz, no tearing.
- [ ] `perch-capture` Linux: portal ScreenCast → PipeWire, per-window streams,
      DMA-BUF import into wgpu (zero copy) with MemFd fallback.
- [ ] Persist the portal restore token so the prompt appears once.
- [ ] Virtual displays on GNOME (`RecordVirtual`) so windows can be moved
      *into the glasses* like extra monitors; wlroots path.
- [ ] Window picker in `perch` CLI and phone app (`enumerate()` → chips).
- [ ] Focus outline + cursor rendering in the compositor.
- [ ] Android: scrcpy/ADB `Source` so the phone/DeX appears as a window on the laptop host.

## M3 — Seamless input
**Done when:** one keyboard+mouse on the laptop drives the laptop windows *and*
the phone window; pushing the pointer off the right edge of the editor moves
into the browser pinned to its right; typing on the phone touchpad lands in
whatever is focused.
- [ ] Linux evdev source thread in `perchd` (`share_local_devices`), grab on
      remote focus, panic key.
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
