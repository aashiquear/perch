# Bose AR headsets — device notes

Used as the head-motion source where the glasses' own sensor cannot be read
(iPadOS). Everything marked *measured* was read from a **Noise Cancelling
Headphones 700** on 2026-11-14 (`.claude/memory/hardware-log.md`); the rest
follows the community description of Bose's withdrawn SDK
([zakaton/Bose-Frames-Web-SDK](https://github.com/zakaton/Bose-Frames-Web-SDK))
and is "very likely" until confirmed. Frames and QC35 II are said to speak
the same protocol; not tested here.

## Bluetooth LE

- Advertises as `LE-Bose NC 700 HP` with services `FDD2` (Bose AR), `FE26`,
  `FE03` *(measured)*. Connects with no pairing prompt, while the headset is
  also the audio device *(measured)*.
- Service `FDD2`, characteristics *(all present on the 700, measured)*:

| characteristic | use |
|---|---|
| `7B61AD83-041C-4333-A0AB-EFB2AB7BDD43` | device information (read, notify) |
| `855CB3E7-98FF-42A6-80FC-40B32A2221C1` | sensor information (read): 12-byte entries, sample-period bitmask at bytes 9–10 |
| `5AF38AF6-000E-404B-9B46-07F77580890B` | sensor configuration (read, write, notify) |
| `56A72AB8-4988-4CC8-A752-FBD1D54A953D` | sensor data (notify) |
| `A0384F52-…`, `21E550AF-…`, `9014DD4E-…` | gestures: information, configuration, data (unused) |

All multi-byte integers are big-endian.

## Sensors

| id | sensor | payload after the 3-byte header |
|---|---|---|
| 0 | accelerometer | x y z `i16`/4096, accuracy `u8` |
| 1 | gyroscope | x y z `i16`/4096, accuracy `u8` |
| 2 | rotation (compass-referenced) | x y z w `i16`/16384, accuracy `u16` |
| 3 | game rotation (gravity-referenced, no compass) | x y z w `i16`/16384 |

The 700 offers ids 0–3, each at 320 / 160 / 80 / 40 / 20 ms *(measured)*.

**Configure:** read the table (`id u8, period u16` per sensor), set one
period, write it all back. `00 00 00 01 00 00 02 00 00 03 00 14` = game
rotation every 20 ms, everything else off; accepted, gives **49 Hz**
*(measured)*.

**Data:** records of `id u8, timestamp u16 (ms, wraps), payload`, **three to
six packed in one notification**, so packets arrive every 60–120 ms
*(measured)*. Time samples by the sensor clock, not by arrival. The first
few samples after enabling start near identity and then jump to the
gravity-referenced value; recentre after that.

## Axes *(measured from three marked poses)*

Sensor world: **Z up**. Sensor body: **X right, Y backward, Z down**.
To Perch's RUB frames: body (x, y, z) → (x, −z, y), world (x, y, z) →
(x, z, −y). The headband sat 15–20° off level on the head, which the
recentre removes together with the arbitrary heading. Encoded as
`perch_tracking::external::SensorFrame::bose_ar()` and checked against the
recorded poses in its tests.

## In the code

`apps/ipad/Sources/PerchCore/BoseAR.swift` (bytes),
`apps/ipad/Sources/Perch/Host/BoseProbe.swift` (CoreBluetooth),
`crates/perch-tracking/src/external.rs` (orientation → head pose).
