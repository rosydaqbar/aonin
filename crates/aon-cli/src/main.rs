use aon::{audio, net};

use anyhow::{Context, Result};
use aon_core::protocol::DEFAULT_PORT;
use clap::{Parser, Subcommand};
use std::net::{SocketAddr, ToSocketAddrs};

#[derive(Parser)]
#[command(name = "aon", about = "Highest-quality real-time audio over the local network")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// List audio devices.
    Devices,
    /// Testing: print what the OS is blocking (permission checks).
    #[command(hide = true)]
    Permissions,
    /// Testing: play a sine tone on an output device.
    #[command(hide = true)]
    Tone {
        #[arg(long)]
        output: Option<String>,
        #[arg(long, default_value_t = 440.0)]
        freq: f32,
        #[arg(long, default_value_t = 10)]
        seconds: u64,
        /// Linear amplitude, 0..1.
        #[arg(long, default_value_t = 0.25)]
        level: f32,
    },
    /// Receive every sender on the network, mix, and play.
    Hub {
        #[arg(long, default_value_t = DEFAULT_PORT)]
        port: u16,
        /// Output device (substring of its name). Default: system default.
        #[arg(long)]
        output: Option<String>,
    },
    /// Become a host through the app engine (discovery, two-way, stats).
    Host {
        /// Output device (substring of its name). Default: system default.
        #[arg(long)]
        output: Option<String>,
        /// Testing: hand the listener role to the first device sending here
        /// after this many seconds, then keep reporting.
        #[arg(long)]
        hand_over_after: Option<u64>,
        /// Listen privately with a code (6 letters and numbers); `new` makes one.
        #[arg(long)]
        private: Option<String>,
        /// The name audio senders see.
        #[arg(long)]
        name: Option<String>,
        /// Testing: turn private (with a new code) after this many seconds.
        #[arg(long, hide = true)]
        private_after: Option<u64>,
        /// Testing: press Stop after this many seconds, then keep running.
        #[arg(long, hide = true)]
        stop_after: Option<u64>,
    },
    /// Join a host found on the network through the app engine.
    Join {
        /// Part of the host's name; the first host seen if omitted.
        name: Option<String>,
        /// Ask the host to stream its own audio back.
        #[arg(long)]
        hear_system: bool,
        /// Ask the host to stream its microphone back.
        #[arg(long)]
        hear_mic: bool,
        /// Don't send this machine's system audio.
        #[arg(long)]
        no_send: bool,
        /// Where to play what comes back (substring). Default: system default.
        #[arg(long)]
        output: Option<String>,
        /// Join the private listener with this code instead of a public one.
        #[arg(long)]
        code: Option<String>,
        /// Seconds to stay connected.
        #[arg(long, default_value_t = 30)]
        seconds: u64,
        /// Testing: turn auto-reconnect on, as the app does.
        #[arg(long)]
        reconnect: bool,
        /// Testing: disconnect (as the app's button does) after this many
        /// seconds, then keep reporting until --seconds.
        #[arg(long)]
        disconnect_after: Option<u64>,
    },
    /// Stream this machine's audio to a hub.
    Send {
        /// Hub address, e.g. 192.168.1.20 or 192.168.1.20:47100
        hub: String,
        #[arg(long, value_enum, default_value = "system")]
        source: audio::Source,
        /// Capture device (substring of its name). Default: system default.
        #[arg(long)]
        device: Option<String>,
        /// Testing: randomly drop this percentage of packets to simulate bad Wi-Fi.
        #[arg(long, default_value_t = 0, hide = true)]
        drop_percent: u32,
        /// highest, cd, balanced or saver.
        #[arg(long, default_value = "highest")]
        quality: String,
        /// Testing: play a quiet tone from this same process on this output
        /// device. System capture should leave it out.
        #[arg(long, hide = true)]
        self_tone: Option<String>,
        /// Silence this computer's speakers while sending (the stream keeps the audio).
        #[arg(long)]
        mute_speakers: bool,
        /// Testing: switch to this hub after --move-after seconds without
        /// stopping capture, as a handover does.
        #[arg(long, hide = true)]
        move_to: Option<String>,
        #[arg(long, default_value_t = 5, hide = true)]
        move_after: u64,
    },
}

fn main() -> Result<()> {
    aon::run_speaker_guard_if_asked();
    match Cli::parse().cmd {
        Cmd::Devices => audio::list_devices(),
        Cmd::Permissions => {
            let c = aon::permissions::Checker::start();
            std::thread::sleep(std::time::Duration::from_millis(1500));
            println!("{:?}", c.get());
            Ok(())
        }
        Cmd::Host { output, hand_over_after, private, name, private_after, stop_after } => {
            let engine = cli_engine(output);
            let code = private.map(|c| if c == "new" { aon::private::new_code() } else { c });
            if let Some(c) = &code {
                println!("private, code {c}");
            }
            engine.start_listening(name, code)?;
            let start = std::time::Instant::now();
            let mut handed = false;
            let (mut made_private, mut stopped) = (false, false);
            loop {
                std::thread::sleep(std::time::Duration::from_millis(250));
                let snap = engine.tick();
                if !made_private && private_after.is_some_and(|s| start.elapsed().as_secs() >= s) {
                    let c = aon::private::new_code();
                    println!("-- private, code {c}");
                    engine.set_private(Some(c))?;
                    made_private = true;
                }
                if !stopped && stop_after.is_some_and(|s| start.elapsed().as_secs() >= s) {
                    println!("-- stop");
                    engine.stop_hosting();
                    stopped = true;
                }
                if !handed
                    && hand_over_after.is_some_and(|s| start.elapsed().as_secs() >= s)
                    && let Some((id, name)) = snap.client_ids().into_iter().next()
                {
                    println!("-- hand over to {name}");
                    engine.hand_over(aon::engine::parse_id(&id).unwrap())?;
                    handed = true;
                }
                if start.elapsed().as_millis() % 2000 < 250 {
                    println!("{:.1}s {}", start.elapsed().as_secs_f32(), snap.summary());
                }
            }
        }
        Cmd::Join { name, hear_system, hear_mic, no_send, output, code, seconds, reconnect, disconnect_after } => {
            let engine = cli_engine(output);
            engine.set_reconnect(reconnect);
            let want = name.map(|n| n.to_lowercase());
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
            if let Some(code) = code {
                // Give discovery a moment to see the listener, then pair.
                std::thread::sleep(std::time::Duration::from_millis(1500));
                engine.tick();
                engine.join_code(&code)?;
                loop {
                    std::thread::sleep(std::time::Duration::from_millis(100));
                    let snap = engine.tick();
                    if snap.is_linked() {
                        println!("joined: {}", snap.summary());
                        break;
                    }
                    if let Some(e) = snap.join_failed() {
                        anyhow::bail!("{e}");
                    }
                }
            } else {
                let host = loop {
                    let snap = engine.tick();
                    let found = snap.host_ids().into_iter().find(|(_, n, ok)| *ok && want.as_ref().is_none_or(|w| n.to_lowercase().contains(w)));
                    if let Some((id, n, _)) = found {
                        println!("found host {n}");
                        break id;
                    }
                    if std::time::Instant::now() > deadline {
                        anyhow::bail!("no matching host on the network ({})", snap.summary());
                    }
                    std::thread::sleep(std::time::Duration::from_millis(250));
                };
                engine.connect(aon::engine::parse_id(&host).unwrap())?;
            }
            if no_send {
                engine.set_send(aon_core::protocol::Source::System, false);
            }
            if hear_system {
                engine.set_hear(aon_core::protocol::Source::System, true)?;
            }
            if hear_mic {
                engine.set_hear(aon_core::protocol::Source::Mic, true)?;
            }
            let start = std::time::Instant::now();
            let mut disconnected = false;
            let mut last_print = start;
            while start.elapsed().as_secs() < seconds {
                std::thread::sleep(std::time::Duration::from_millis(100));
                let snap = engine.tick();
                if last_print.elapsed().as_secs() < 1 {
                    continue;
                }
                last_print = std::time::Instant::now();
                if !disconnected && disconnect_after.is_some_and(|d| start.elapsed().as_secs() >= d) {
                    println!("-- disconnect");
                    engine.forget_host();
                    disconnected = true;
                }
                println!("{:.1}s {}", start.elapsed().as_secs_f32(), snap.summary());
            }
            engine.shutdown();
            Ok(())
        }
        Cmd::Tone { output, freq, seconds, level } => {
            let _stream = audio::play_tone(output.as_deref(), freq, level)?;
            std::thread::sleep(std::time::Duration::from_secs(seconds));
            Ok(())
        }
        Cmd::Hub { port, output } => {
            let follow = output.is_none();
            net::run_hub(port, || audio::start_output(output.as_deref()), follow, &net::HubShared::default(), true)
        }
        Cmd::Send { hub, source, device, drop_percent, self_tone, quality, mute_speakers, move_to, move_after } => {
            let hub = resolve(&hub)?;
            let _tone = self_tone.map(|out| audio::play_tone(Some(&out), 440.0, 0.01)).transpose()?;
            let (_stream, rx, rate) = audio::start_capture(source, device.as_deref(), mute_speakers)?;
            let os = if cfg!(target_os = "macos") { aon_core::protocol::Os::MacOs } else { aon_core::protocol::Os::Windows };
            let name = gethostname::gethostname().to_string_lossy().into_owned();
            let who = net::Identity { device: std::process::id() as u64, name, os };
            let kind = match source {
                audio::Source::System => aon_core::protocol::Source::System,
                audio::Source::Mic => aon_core::protocol::Source::Mic,
            };
            let quality = aon_core::protocol::Quality::from_name(&quality).ok_or_else(|| anyhow::anyhow!("quality must be highest, cd, balanced or saver"))?;
            let cfg = net::SendConfig { hub, who, source: kind, quality, drop_percent, verbose: true, cipher: None };
            let shared = std::sync::Arc::new(net::SendShared::default());
            if let Some(to) = move_to {
                let (to, s) = (resolve(&to)?, shared.clone());
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_secs(move_after));
                    println!("-- moving to {to}");
                    *s.retarget.lock().unwrap() = Some(to);
                });
            }
            net::run_sender(cfg, rx, rate, &shared)
        }
    }
}

/// An engine for CLI testing: random identity, no persisted state.
fn cli_engine(output: Option<String>) -> std::sync::Arc<aon::engine::Engine> {
    use std::hash::{BuildHasher, RandomState};
    let id = RandomState::new().hash_one(std::time::Instant::now()) | 1;
    let name = gethostname::gethostname().to_string_lossy().into_owned();
    let prefs = aon::engine::Prefs { reconnect: false, output, ..Default::default() };
    aon::engine::Engine::new(id, name, prefs, aon_pro::new())
}

fn resolve(hub: &str) -> Result<SocketAddr> {
    let with_port = if hub.contains(':') { hub.to_string() } else { format!("{hub}:{DEFAULT_PORT}") };
    with_port.to_socket_addrs()?.next().with_context(|| format!("cannot resolve {hub}"))
}
