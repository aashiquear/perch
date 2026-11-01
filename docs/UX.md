# UX principles

The product is "my laptop desk, but in my glasses, with no desk". Everything
below follows from three facts: your hands are on a keyboard, your phone is in
your pocket or lying flat, and the field of view is ~46° — one window at a
time is sharp, the rest is peripheral.

## Defaults that must feel right on first run

1. **Plug in → it works.** No menus: glasses detected, mono 120 Hz set, last
   layout restored, or `single` preset dead ahead if none. Uncalibrated is
   fine for the first minute; a quiet toast suggests calibration later.
2. **Recenter is one physical press** (temple button 0) and one tap on the
   phone. It never changes pitch — a window at eye level stays at eye level.
3. **Pin where I'm looking.** Look at the spot, tap "Pin here". No dragging
   windows around a 3D scene with a mouse.
4. **Presets, not sliders.** `coding` (centre + two angled side panels at
   ±31°), `single`, `gaming` (head-locked, max size), `couch` (pitched down
   15°, smooth follow). Save your own with one tap.
5. **Focus follows your intent, never surprises you.** Explicit focus: chips,
   `Next`, three-finger swipe. Edge switching needs 40 px of resistance so a
   flick doesn't jump windows. Look-to-focus is opt-in with a dwell time.
6. **One keyboard.** Whatever is focused gets the keys, on whatever machine.
   Scroll Lock (configurable) is the panic key: input returns to local.
7. **Black is transparent.** No wallpaper, no chrome. The only lit pixels are
   windows and a 2 px focus outline.

## Phone touchpad vocabulary (matches macOS/Windows precision touchpads)

| Gesture | Action |
|---|---|
| 1-finger move | pointer (accelerated) |
| 1-finger tap | left click |
| double-tap + hold | drag |
| 2-finger tap | right click |
| 2-finger drag | scroll (natural) |
| 3-finger tap | middle click |
| 3-finger swipe ←/→ | previous / next window |
| text field | types into focused window (delta-forwarded) |
| chip tap / long-press | focus / pin here |

## Calibration UX

"Put the glasses on the table, don't touch them for 30 s." Progress in
percent. Rejected if moved; the previous calibration stays. Done = saved per
serial, never asked again unless drift is noticed.

## Latency budget (target, mono 120 Hz)

IMU 1 ms → fusion 0.01 ms → predict to scan-out (+13 ms device, +4 ms margin)
→ render 2–4 ms → present (Mailbox) 0–8 ms → panel 13 ms. Perceived error
≈ prediction error only, which at 2 rad/s and ±4 ms is ~0.5°.

## Things we deliberately don't do (yet)

- 6DOF / hand tracking (hardware can't).
- Curved mega-screen — the Breezy-style single huge canvas is nice for video;
  for work, discrete windows you can pin are better. Can be a preset later.
- Drawing a 3D "room".

## The slot grid (since 1.2)

Windows snap to a grid of cells on a sphere around the head: `cols × rows`
cells of `cell_w × cell_h` metres with `gap` between them, `distance` away,
centred at (`yaw`, `pitch`). Defaults: 3 × 1, 1.2 × 0.8 m, 0.1 m gap, 2 m.
Each cell centre is wrapped onto the sphere, so a 3-wide grid curves around
you rather than flattening at the edges. A window keeps its aspect ratio
and is fitted inside its cell.

- `perch pick` drops each new window into the next free slot (row-major);
  once the grid is full, windows fan out to the right of where you looked.
- `perch grid` prints the grid; `perch grid --cols 3 --rows 2 --cell-w 1.1
  --cell-h 0.7 --gap 0.08 --distance 2 --yaw 0 --pitch -5 --arrange`
  changes it and snaps everything in.
- `perch slot <id> 2 0` moves a window to column 2, row 0 (evicting the
  previous occupant from the slot, not from space).
- `perch place` / `perch pin` take a window *out* of the grid.
- The grid is saved with the workspace.
