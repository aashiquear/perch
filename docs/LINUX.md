# Perch on Linux — options, shortcuts, commands

Everything the Linux host (`perchd`) and its CLI (`perch`) understand, in one
place. For the first-time setup see the README; for the feel of it see
[UX.md](UX.md).

## Keyboard shortcuts (perchd, GNOME)

Chords use **Ctrl+Alt** on either side and fire when both are released, so
GNOME never sees them. All of them are configurable (see `[input]`).

| keys | action |
|---|---|
| **Ctrl+Alt+G** | move the laptop's focused app window to a new full-size glasses display (1920×1080) |
| **Ctrl+Alt+H** | same, half width (960×1080): two side by side |
| **Ctrl+Alt+P** | glasses mode on/off: mouse and keyboard go to the glasses (off = laptop mode) |
| **Scroll Lock** | same as Ctrl+Alt+P (the "panic key"; keyboards without it use the chord) |
| **Esc** | cancel a pending "To glasses" from the dashboard |
| temple button | re-centre (configurable: `[buttons]`) |

How a move works: perchd creates a virtual monitor (GNOME appends it to the
right of the existing ones: laptop, glasses panel, then one per window), then
injects GNOME's own `move-to-monitor-right` shortcut once per monitor in
between. During the add it briefly leaves fullscreen on the glasses, because
GNOME 50 otherwise drops a fullscreen window on a secondary monitor onto the
laptop and focuses it, which would make the shortcut move Perch's own window.

In glasses mode the touchpad moves a pointer drawn on the glasses, two
fingers scroll (direction per `natural_scroll`), and every key goes to the
app under the focused window. Windows are placed in a flow-row grid; drag a
window with the pointer to re-arrange (snap lines appear), or use the bar.

## The bar on the glasses

A strip under the focused window; hover for the label.

| tile | does |
|---|---|
| RECENTER | current gaze becomes straight ahead |
| CENTRE HERE | put the grid where you are looking |
| ARRANGE | re-flow every window into the grid |
| FEWER ROWS / MORE ROWS | change the grid's row count |
| NEARER / FARTHER | move the whole grid along your gaze |
| TO GLASSES | the laptop's focused app → a new glasses display (Ctrl+Alt+G) |
| TO LAPTOP | the focused window's app goes back to the laptop |
| FULL / HALF WIDTH | re-create the focused window's display at the other width |
| CLOSE APP | close the application, then remove the window |
| CALIBRATE | 30 s stationary gyro calibration |
| ALL TO LAPTOP | remove every window |
| GLASSES MODE | toggle input routing (Ctrl+Alt+P) |

## The dashboard (laptop window)

Header with the glasses picture, model, display mode and calibration state,
and the mode line (GLASSES MODE / LAPTOP MODE). Stat tiles: windows, rows
used, distance, fps, peers. A layout map (yaw → x, pitch → y). Square
actions: To glasses, To glasses half width, Glasses mode, All to laptop,
Recenter, Centre grid here, Arrange (presets as pictures: 1×1, 2 side by
side, 3, wide centre, 2×2, re-flow), Calibrate. Sound & input: default output
(one-click glasses ⇄ laptop), natural scrolling. Window cards: focus, Pin
here, size, span, To laptop, Close app. Advanced: grid sliders (rows,
distance, row height, px per m, block width, gap) and Sharing (this host's
address, peers).

## Configuration — `~/.config/perch/perchd.toml`

Written with defaults on first start (`--config-dir` overrides the folder).
Values shown are the defaults.

```toml
name = "<hostname>"            # how phones see this host
pairing_token = "…"            # phones must present it (printed on first start)
control_bind = "0.0.0.0:7420"  # WebSocket control
stream_bind  = "0.0.0.0:7421"  # UDP poses + input
mdns = true                    # advertise _perch._tcp for phones to find

[display]
monitor_name = "Rokid"         # substring of the GNOME monitor/connector to render on (e.g. "DP-5")
fov_h_deg = 46.0               # horizontal field of view used for projection; tune until sizes look right
tilt_deg = 4.0                 # display tilt relative to the IMU
mode = "mono120"               # mono | stereo | mono120 | stereo120 (requested on connect)
ipd_m = 0.064                  # inter-pupillary distance for stereo
extra_prediction_ms = 4.0      # extra pose prediction on top of the display delay (13 ms)

[tracker]
kp = 1.0                       # gravity correction gain
ki = 0.02                      # bias learning from gravity (roll/pitch only)
accel_gate = 0.15              # ignore accelerometer samples further than this fraction from g
use_mag = false                # magnetometer yaw correction (off: indoor fields are unreliable)
max_dt = 0.1                   # skip integration over gaps longer than this (s)
calib_max_rate_deg_s = 5.0     # reject a calibration run if the glasses moved faster than this
auto_bias = true               # keep learning the gyro bias whenever the glasses are still
auto_bias_max_rate_deg_s = 1.5 # "still" threshold
auto_bias_window_s = 2.0       # seconds of stillness per update
auto_bias_tau_s = 15.0         # time constant of the update

[input]
share_local_devices = true     # grab the laptop's keyboard/touchpad in glasses mode (needs the `input` group)
edge_resistance_px = 40.0      # pointer resistance before crossing between windows
gaze_focus_ms = 0              # look-to-focus dwell (0 = off; focus follows clicks/bar)
panic_key_usage = 0x47         # HID usage that toggles glasses mode (Scroll Lock)
grab_chord_usage = 0x13        # Ctrl+Alt + this toggles glasses mode (P)
move_chord_usage = 0x0a        # Ctrl+Alt + this moves the focused app to the glasses (G)
move_half_chord_usage = 0x0b   # Ctrl+Alt + this: half-width display (H)
natural_scroll = true          # two-finger scroll: content follows the fingers

[buttons]
button0 = "recenter"           # temple button 0: recenter | focus_next | none
button1 = "focus_next"

[capture]
restore_token = ""             # portal restore token (other Wayland compositors); cleared by `perch pick`
fps = 60                       # requested capture frame rate
```

HID usages: letters are 0x04 (A) … 0x1d (Z); see `perch-input/src/keymap.rs`.

## `perchd` flags

| flag | meaning |
|---|---|
| `--config-dir DIR` | use `DIR/perchd.toml` and `DIR/calibration.json` instead of `~/.config/perch` |
| `--mock synthetic` | no glasses: a still synthetic IMU (develop the panel/compositor) |
| `--mock FILE.jsonl` | replay a recorded IMU file at its own pace |
| `--headless` | no glasses rendering and no dashboard (server + CLI only) |

## `perch` (CLI)

| command | does |
|---|---|
| `status` | glasses, tracking, windows, peers |
| `recenter` | current gaze yaw = 0 |
| `calibrate --seconds N` | stationary gyro calibration (glasses on a table) |
| `mode mono\|stereo\|mono120\|stereo120` | display mode |
| `move [--width W --height H]` | the host's focused app → a new glasses display |
| `remove ID` (alias `move-back`) | window back to the laptop |
| `close-app ID` | close the app, then remove the window |
| `resize ID W H` | re-create the window's display at a new size |
| `clear` | remove every window |
| `windows` | list windows |
| `focus ID\|next` | focus |
| `pin ID` | pin where you are looking |
| `place ID --yaw … --pitch … --distance … --width … --height …` | explicit placement (degrees, metres) |
| `anchor ID world\|head\|smooth_follow` | anchoring |
| `grid [--rows N] [--distance M] [--arrange] …` | the flow-row grid; `arrange` snaps every window into it |
| `slot ID ROW INDEX`, `size ID W H`, `span ID BLOCKS` | per-window grid control |
| `preset single\|coding\|gaming\|couch` | layouts |
| `layout save\|load NAME` | save / restore a layout |
| `pick [--multiple]` | portal window picker (non-GNOME compositors) |
| `watch` | print events as JSON lines |
| `record FILE --seconds N` | record raw IMU samples (`--features rokid`) |
| `discover` | find perchd hosts via mDNS |

## Phone and iPad as a remote

The Android app's **Laptop remote** tab (and the iPad app) finds this host
over mDNS and offers the same pop-up as the phone-as-host mode: touchpad
(acts as a mouse), arrow pad, keyboard bar, and the Xbox / wheel / flight
skins, which perchd presents to the laptop as a virtual Xbox 360 controller
(`/dev/uinput`, "Perch Virtual Gamepad"), so Steam, browsers and emulators
see a real pad. Window chips focus, Pin here pins, and the Layout chip
applies presets.
