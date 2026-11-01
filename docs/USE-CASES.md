# Use cases — how to do each thing with Perch today

Each section is a recipe for a real session. Linux/GNOME is the host that
works end to end (v0.1.0); the Android host is in development (see
[ANDROID.md](ANDROID.md)). Keyboard chords are the Linux defaults
(`[input]` in `perchd.toml`).

## 1. Coding on the laptop with several windows around you

1. `perchd --config-dir ./dev`; put the glasses on; `perch calibrate --seconds 30`
   once with the glasses on the table (0.08°/min drift afterwards).
2. Focus the editor, **Ctrl+Alt+G** → it becomes a 1920×1080 glasses display
   straight ahead. Focus the terminal, **Ctrl+Alt+H** → a 960-wide display
   beside it. Add the browser with **Ctrl+Alt+G**.
3. **Ctrl+Alt+P** → glasses mode: the trackpad pointer moves freely across the
   three displays, keys go to whichever you clicked last. Alt+drag a window to
   re-order it; the bar's FEWER/MORE ROWS puts them on two rows.
4. Look away and back: windows stay where you pinned them. RECENTER (temple
   button) if the world has drifted after an hour.
5. **Ctrl+Alt+P** again to use the laptop screen; the apps stay on the glasses.
   Done: TO LAPTOP on each, or Quit, and every app comes back.

Dashboard alternative: "To glasses" → click the app window; presets under
Arrange (wide centre, 2×2, …) re-size displays to full/half for you.

## 2. Watching a film or video

1. Open the player, **Ctrl+Alt+G**, then in glasses mode press F (or the
   player's full-screen) — the display is exactly 1920×1080, so the video
   fills the window edge to edge.
2. Bar → FARTHER / NEARER to set the apparent screen size; `perch grid
   --distance 3` for a cinema-sized window.
3. Dashboard → **Sound output → glasses**: audio goes to the glasses'
   speakers (the GPU's DisplayPort audio) while the laptop stays silent. Flip
   it back for headphones on the laptop.
4. Brightness: the rocker on the right temple.
5. Lean back: windows are world-anchored, so lie down and look up — the film
   stays where you left it. CENTRE HERE moves the whole grid to the new gaze.

## 3. Gaming

1. Launch the game windowed, **Ctrl+Alt+G**. Most games honour the display
   size and run at 1920×1080 on the glasses display.
2. Glasses mode for the pointer; the keyboard is grabbed at the evdev level,
   so WASD/modifiers reach the game unchanged. Scroll direction is a panel
   toggle. Two-finger scrolling and tap-to-click are interpreted by Perch's
   own touchpad code, so a mouse is better for shooters: plug one in and it is
   grabbed too.
3. Prefer a head-locked HUD-style screen? `perch place <id> --anchor head`
   keeps it in front of you while you move; world-anchored is the default.

## 4. Reading / reference beside the work

Send a PDF viewer or docs tab with **Ctrl+Alt+H** (half width) and leave it to
the side; MORE ROWS puts long documents on a second row above the editor.
Windows are only re-arranged when you ask (ARRANGE / presets); they never
jump.

## 5. Presentations and sharing

The glasses are a private screen; the laptop keeps showing what the audience
sees. Keep notes on the glasses (Ctrl+Alt+H), slides on the projector. The
phone/iPad touchpad app (mDNS, no pairing beyond the token) lets you drive
the laptop from the sofa.

## 6. Phone as host (Android, in development)

The phone drives the glasses alone: USB-C video + IMU, Perch's Android app
renders the same 3DOF windows. Choose apps from the Perch panel; each runs
in its own glasses display, portrait or landscape; a pop-up touchpad on the
phone moves a cursor in the focused window. See [ANDROID.md](ANDROID.md) for
what works without root and what needs Shizuku.
