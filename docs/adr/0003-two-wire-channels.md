# ADR 0003: JSON control channel + postcard stream channel

**Status:** accepted · 2026-10-31

## Decision
Low-rate control/state as internally-tagged JSON over WebSocket (debuggable,
trivial for Kotlin/Swift/JS). High-rate poses/input as externally-tagged
postcard over UDP (≤64 B per pose, hand-encodable in ~40 lines, loss-tolerant).

## Consequence
`InputEvent` must be externally tagged (postcard limitation). Byte-exact tests
exist in all three languages.
