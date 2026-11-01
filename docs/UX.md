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
   Keyboards without Scroll Lock (Apple) use Ctrl+Alt+P (`grab_chord_usage`).
   Laptop touchpads with no relative axes are interpreted in Rust
   (`perch-input::touchpad`): one finger moves, tap clicks (two-finger tap =
   right), two fingers scroll naturally, physical click with two fingers =
   right button.
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

## The layout grid (since 1.2, re-shaped in 1.3)

Windows live in **rows** on a sphere around the head. A row is a flow
layout: its windows sit side by side in order with a gap between them,
centred on the layout centre, and **each window's width comes from its own
pixel size** (`px_per_m`, default 1600 px/m: a 1920-px browser is 1.2 m, a
960-px terminal 0.6 m), clamped to `max_w_m` and to the row height. Rows
stack `row_h_m + gap_m` apart around `center_pitch`, row 0 on top.
Defaults: 1 row, 0.8 m high, 0.08 m gap, 2 m away, centred straight ahead.

- `perch pick` appends each new window to the emptiest row.
- `perch grid` prints the grid; `perch grid --rows 2 --row-h 0.7 --distance
  2.5 --px-per-m 1400 --arrange` changes it and re-flows everything.
- `perch slot <id> <row> <index>` moves a window; `perch size <id> W H`
  overrides its pixel size; `perch arrange` re-flows all windows.
- `perch place` / `perch pin` take a window *out* of the grid (its row
  closes up).
- Saved with the workspace.

## Pointer on the glasses (since 1.3)

When the OS pointer is on the glasses output, perchd draws its own arrow and:

- **click** a window → focus it;
- **drag** a window → it leaves its row (the row closes up) and follows the
  pointer; while over a row, a vertical **insertion guide** and ghost
  outlines show where every window in that row would settle; release to
  drop it there, or release outside any row to leave it free-floating;
- the **control bar** along the bottom (always visible, head-locked):
  RECENTER (gaze → yaw 0), CENTRE HERE (layout centre → current gaze),
  ARRANGE, ROWS −/+, NEARER/FARTHER, PICK, CALIBRATE, CLEAR.

Clicks *inside* a captured window do not yet reach the application behind
it (that is M3: input routing).

## Control bar, panel and input grab (since 1.4)

- **Bar**: a compact pill of icon tiles at the bottom centre of the glasses;
  hovering a tile shows its name above the pill. A chip at the right shows
  the clock, open windows, rows, distance and who owns the mouse (OS/PERCH).
- **Control panel**: `perchd` also opens a normal window on the laptop
  ("Perch"): pick windows, arrange, recenter, centre here, calibrate, clear,
  the grid sliders, per-window focus/pin/remove/span, sharing status, quit.
- **Grab**: the GRAB tile, the panel checkbox or **Scroll Lock** hands the
  physical mouse and keyboard to Perch (needs the `input` group). The
  pointer then lives on the glasses: over a captured *monitor* it is placed
  at the matching desktop position and clicks/scroll/keys go to the
  application there; over a captured *window* motion is relative (the
  portal does not reveal where a window sits); Alt + left button drags the
  spatial window instead; the bar works as usual. Scroll Lock again gives
  the devices back to the OS.
- **Spans**: `perch span <id> 2` makes a window two layout blocks wide
  (`--block-w`), whatever its pixel size; `0` returns to automatic.

## Glasses displays on GNOME (since 1.4, replaces window capture)

On GNOME each spatial window is a **virtual monitor** (Mutter `RecordVirtual`),
1920×1080 by default, created to the right of the physical displays. The
app lives only on the glasses; when its spatial window is removed, or the
daemon exits, the monitor disappears and GNOME moves the app back to the
laptop. Nothing persists across restarts: send windows again.

- **Send the focused app**: Ctrl+Alt+G (`send_chord_usage`), or the SEND
  WINDOW tile / panel button (those move whatever is focused, which may be
  Perch's own panel). The daemon injects GNOME's `move-to-monitor-right`
  shortcut as many times as monitors lie to the right of the laptop.
- **Pointer**: while grabbed, the spatial pointer is placed absolutely inside
  the hovered glasses display through Mutter's RemoteDesktop session, so it
  moves freely between glasses windows and never clamps. Releasing the grab
  (Ctrl+Alt+P) throws the OS pointer back onto the leftmost physical display.
- **Keyboard** goes to the focused app; clicking a glasses window focuses it.
- Pointer updates are coalesced per batch and sent without waiting for
  replies, which is what removed the lag of the portal-based first attempt.
