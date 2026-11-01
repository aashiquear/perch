# Perch for iPad / iPhone

A controller for a `perchd` host: big trackpad, keyboard forwarding, window
chips, presets, display mode. iPadOS can't access the glasses' USB IMU, so
there is no host mode here — plug the glasses into your laptop or Android.

## Build

```bash
brew install xcodegen
cd apps/ipad && xcodegen generate && open Perch.xcodeproj
```

Targets iOS 16+. Uses `NWBrowser` (Bonjour `_perch._tcp`),
`URLSessionWebSocketTask` (control) and `NWConnection` UDP (input). Unit test
`PostcardTests` pins the byte format against Rust.

## Use

Open the app on the same Wi-Fi as the laptop → tap the host → trackpad.
Three-finger swipe switches windows; long-press a chip to pin that window
where you're looking. Attach a keyboard to the iPad and type into whatever is
focused on the laptop.
