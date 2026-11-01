# ADR 0001: One Rust core, thin platform shells

**Status:** accepted · 2026-10-31

## Context
Five platforms (Linux, Android, Windows, macOS, iPadOS) and one fragile piece
of maths (IMU fusion + projective window placement + input routing) that must
behave identically everywhere.

## Decision
All logic in Rust crates (`perch-tracking`, `perch-space`, `perch-input`,
`perch-proto`), exposed to Kotlin/Swift through a C ABI (`perch-ffi`).
Platform code only does I/O: USB fd, surfaces, gestures, injection.

## Alternatives
- Kotlin Multiplatform / Flutter: good UI story, poor fit for 1 kHz sensor
  loops and uinput/wgpu.
- Separate native implementations: triple the bugs in the maths.

## Consequences
- Android needs cargo-ndk + a JNI shim; iPad doesn't link Rust today.
- UI frameworks stay native (Compose, SwiftUI), which is what users expect.
