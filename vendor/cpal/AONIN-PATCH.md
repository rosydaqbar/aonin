# Vendored cpal 0.18.2

Unmodified except for `src/host/coreaudio/macos/loopback.rs`: the loopback
process tap is exclusive, and upstream passes an empty process list (record
every process). Aonin passes its own Core Audio process object instead, so
system-audio capture never includes what Aonin itself plays. Search for
"Aonin patch" to find the change.

Licensed under Apache-2.0 (see LICENSE), as upstream.
