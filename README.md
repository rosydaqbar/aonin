# Aonin

Hear every computer on your desk through one pair of headphones.

Aonin joins the audio of computers on the same local network into one output. One machine becomes the **host**; the others connect as **clients** and stream their system audio and/or microphone to it, and the host mixes everything and plays it on one device. It runs on **Windows and macOS** from the tray / menu bar.

- **Pure local network.** No servers, no accounts. Devices find each other on the LAN and nothing leaves it.
- **Highest quality by default.** Uncompressed 24-bit / 48 kHz PCM, with lighter tiers (CD 16-bit, and Opus Balanced / Saver) for weak networks.
- **Real time over Wi-Fi.** Typically 15–30 ms of buffering, with redundant packets and retransmits so Wi-Fi packet loss doesn't become dropouts.
- **No drivers.** System audio is captured with WASAPI loopback on Windows and the Core Audio process tap on macOS 14.2+.

> **Status:** early development. The engine and the host/client flow work; see [Roadmap](#roadmap) for what's still missing. There are no release builds yet.

## How it works

```
 MacBook ──┐
 MacBook ──┼── Wi-Fi / Ethernet (UDP, LAN only) ──► Host ──► headphones / speakers
 Laptop  ──┘
```

- **Discovery:** every running Aonin broadcasts a small beacon on the LAN once a second, so hosts appear on their own.
- **Transport:** audio is cut into 2 ms blocks. Every packet also carries the previous block, so a single lost packet is recovered from the next one with no extra delay. Longer gaps are re-requested (NACK) and retransmitted, up to three times.
- **Jitter buffer:** each incoming stream has an adaptive buffer that grows to cover the worst recent Wi-Fi stall and shrinks again while the network behaves. Clock drift between machines is corrected by resampling a few hundred ppm, not by dropping samples.
- **No loops:** a device can't connect to a host whose audio already comes from that device (for example, a host that is itself a client of this machine), because the sound would bounce back and forth. Such hosts are shown but can't be joined.

## Platform notes

- **macOS** asks once for permission to record system audio and the microphone. Until it is granted, capture is silent.
- **Windows Firewall** asks once to allow incoming connections the first time a machine hosts.
- For the steadiest Wi-Fi on a Mac, AirDrop/Handoff (AWDL) can cause short stalls; the adaptive buffer absorbs them at the cost of a few ms.

## Installing

Download the installer for your system from [Releases](https://github.com/rosydaqbar/aonin/releases).

The builds are not code-signed yet, so the first launch needs one extra step:

- **Windows:** SmartScreen may say "Windows protected your PC". Click **More info**, then **Run anyway**. Choose "install for all users" to have the installer allow Aonin through Windows Firewall.
- **macOS:** open the DMG and drag Aonin to Applications. If macOS says it can't be opened, right-click Aonin in Applications and choose **Open**, or run `xattr -cr /Applications/Aonin.app`. Then allow audio recording when asked.

## Building from source

Requirements:

- [Rust](https://rustup.rs) (stable)
- [Node.js](https://nodejs.org) 20 or newer
- [CMake](https://cmake.org) (builds the bundled Opus codec). On Windows the copy in Visual Studio Build Tools works if it is on `PATH`; on macOS, `brew install cmake`
- Windows: Visual Studio Build Tools with the C++ workload (WebView2 ships with Windows 11)
- macOS: Xcode Command Line Tools, macOS 14.2 or newer

```sh
cd app
npm install
npm run tauri dev     # run the app
npm run tauri build   # installer: .exe / .msi on Windows, .app / .dmg on macOS
```

### Command-line tool

The engine also ships as a small CLI, handy for testing without the app:

```sh
cargo run --release -p aon -- devices                 # list audio devices
cargo run --release -p aon -- hub                     # receive, mix and play
cargo run --release -p aon -- send 192.168.1.20       # send this machine's system audio to a hub
cargo run --release -p aon -- send 192.168.1.20 --source mic
```

### Project layout

| Path | What |
| --- | --- |
| `crates/aon-core` | Platform-independent core: wire protocol, jitter buffer, resampler |
| `crates/aon` | Audio I/O (cpal), UDP sender and hub, discovery, host/client engine, CLI |
| `app` | Desktop app: Tauri 2 shell (tray, popover, window) and the Svelte UI |
| `app/icons-src` | Icon sources (SVG) |

Run the tests with `cargo test`.

## Roadmap

- [x] Highest-quality streaming with redundancy, retransmit and an adaptive jitter buffer
- [x] System audio and mic capture on Windows and macOS, no drivers
- [x] LAN discovery, host/client roles, loop detection, auto-reconnect
- [x] Tray / menu bar app with per-device volume, mute and live meters
- [x] Quality tiers: Highest, CD (16-bit), Balanced and Saver (Opus)
- [x] Two-way audio: hear the host's audio and mic on a client
- [x] Never records Aonin's own playback (no echo, no loops)
- [x] Pick audio devices; recover automatically when the output is unplugged or changes
- [x] Open at login, and an optional "keep Wi-Fi steady" switch on macOS (turns off AWDL)
- [x] Installers built by GitHub Actions (Windows NSIS, macOS DMG)
- [x] Automatic updates, signed
- [ ] Hand the host role to another device from the host, with every client following it
- [ ] Private connections: pair devices with a one-time code, with encrypted audio; an optional account later
- [ ] Android and iOS: listen and send the mic first, then host, then system audio where the OS allows it
- [ ] Later: code signing for Windows (e.g. SignPath for open source) and macOS (Apple Developer ID + notarization)

See [docs/roadmap.md](docs/roadmap.md) for how each one will be built.

## License

Aonin is free software, released under the [GNU General Public License v3.0](LICENSE) or (at your option) any later version. You may use, study, share and modify it; if you distribute a modified version, it must stay under the same license with its source available.
