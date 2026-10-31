# Vendored cpal 0.18.2

Two changes, all macOS-only; search for "Aonin patch" to find them:

1. `src/host/coreaudio/macos/loopback.rs`: the loopback process tap is
   exclusive, and upstream passes an empty process list (record every
   process). Aonin passes its own Core Audio process object instead, so
   system-audio capture never includes what Aonin itself plays.
2. `aonin_force_loopback()` (`src/lib.rs`, `macos/mod.rs`, `macos/device.rs`):
   upstream records a device's input whenever it has one, so a virtual
   loopback output such as BlackHole is read through its input and the
   exclusion above never applies. When set on the calling thread, input
   streams always go through the process tap.

Licensed under Apache-2.0 (see LICENSE), as upstream.
