#!/usr/bin/env python3
"""Record raw touchpad frames for a few seconds (diagnostic for perch-input).

Usage: python3 tools/padrec.py [seconds]   (needs the `input` group)
Prints one line per SYN_REPORT: time, finger tools, touch, x, y, buttons.
"""
import glob, os, select, struct, sys, time

SECS = float(sys.argv[1]) if len(sys.argv) > 1 else 8.0
EV_KEY, EV_ABS, EV_SYN = 1, 3, 0
NAMES = {0x110: "BTN_LEFT", 0x145: "FINGER", 0x14a: "TOUCH", 0x14d: "DOUBLE", 0x14e: "TRIPLE", 0x14f: "QUAD"}

def name(path):
    with open(f"/sys/class/input/{os.path.basename(path)}/device/name") as f:
        return f.read().strip()

def caps_abs(path):
    with open(f"/sys/class/input/{os.path.basename(path)}/device/capabilities/abs") as f:
        return int(f.read().split()[-1], 16)

pads = [p for p in glob.glob("/dev/input/event*") if "Perch" not in name(p) and caps_abs(p) & 3 == 3]
if not pads:
    sys.exit("no absolute touch device found")
path = pads[0]
print(f"recording {name(path)} ({path}) for {SECS}s: move one finger right, then down, then two fingers down")
fd = os.open(path, os.O_RDONLY | os.O_NONBLOCK)
fmt = "llHHi"; sz = struct.calcsize(fmt)
state = {"x": None, "y": None, "keys": set()}
t_end = time.time() + SECS; t0 = None; frames = 0
while time.time() < t_end:
    r, _, _ = select.select([fd], [], [], 0.1)
    if not r:
        continue
    buf = os.read(fd, sz * 64)
    for i in range(0, len(buf) - sz + 1, sz):
        sec, usec, typ, code, val = struct.unpack(fmt, buf[i:i + sz])
        if typ == EV_ABS and code == 0: state["x"] = val
        elif typ == EV_ABS and code == 1: state["y"] = val
        elif typ == EV_KEY and code in NAMES:
            (state["keys"].add if val else state["keys"].discard)(NAMES[code])
        elif typ == EV_SYN and code == 0:
            t = sec + usec / 1e6; t0 = t0 or t; frames += 1
            if frames <= 400:
                print(f"{t - t0:7.3f} {' '.join(sorted(state['keys'])):28s} x={state['x']} y={state['y']}")
print(f"{frames} frames")
