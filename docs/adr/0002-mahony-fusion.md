# ADR 0002: Mahony AHRS with stationary bias calibration

**Status:** accepted · 2026-10-31

## Context
3DOF pinning lives or dies on yaw drift. Accelerometer fixes pitch/roll; yaw
has no reference except a noisy magnetometer.

## Decision
Mahony complementary filter (kp=1, ki=0.02) + one-shot stationary gyro-bias
calibration persisted per serial + constant-velocity prediction to scan-out.
Magnetometer correction implemented but off by default.

## Alternatives
Madgwick (similar, less direct bias handling), ESKF (`eskf` crate — more
accurate with good mag, harder to tune, overkill without position).

## Consequences
Expect slow residual yaw drift (≈ minutes to a degree); "recenter" is a
first-class gesture. Re-evaluate at M1 with real recordings.
