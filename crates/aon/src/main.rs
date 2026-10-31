use aon::{audio, net};

use anyhow::{Context, Result};
use aon_core::protocol::DEFAULT_PORT;
use clap::{Parser, Subcommand};
use std::net::{SocketAddr, ToSocketAddrs};

#[derive(Parser)]
#[command(name = "aon", about = "Lossless real-time audio over the local network")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// List audio devices.
    Devices,
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
        /// Seconds to stay connected.
        #[arg(long, default_value_t = 30)]
        seconds: u64,
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
        /// Testing: play a quiet tone from this same process on this output
        /// device. System capture should leave it out.
        #[arg(long, hide = true)]
        self_tone: Option<String>,
    },
}

fn main() -> Result<()> {
    match Cli::parse().cmd {
        Cmd::Devices => audio::list_devices(),
        Cmd::Host { output } => {
            let engine = cli_engine(output);
            engine.become_host()?;
            loop {
                std::thread::sleep(std::time::Duration::from_secs(2));
                println!("{}", engine.tick().summary());
            }
        }
        Cmd::Join { name, hear_system, hear_mic, no_send, output, seconds } => {
            let engine = cli_engine(output);
            let want = name.map(|n| n.to_lowercase());
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
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
            if no_send {
                engine.set_send(aon_core::protocol::Source::System, false);
            }
            if hear_system {
                engine.set_hear(aon_core::protocol::Source::System, true)?;
            }
            if hear_mic {
                engine.set_hear(aon_core::protocol::Source::Mic, true)?;
            }
            for _ in 0..seconds / 2 {
                std::thread::sleep(std::time::Duration::from_secs(2));
                println!("{}", engine.tick().summary());
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
        Cmd::Send { hub, source, device, drop_percent, self_tone } => {
            let hub = resolve(&hub)?;
            let _tone = self_tone.map(|out| audio::play_tone(Some(&out), 440.0, 0.01)).transpose()?;
            let (_stream, rx, rate) = audio::start_capture(source, device.as_deref())?;
            let os = if cfg!(target_os = "macos") { aon_core::protocol::Os::MacOs } else { aon_core::protocol::Os::Windows };
            let name = gethostname::gethostname().to_string_lossy().into_owned();
            let who = net::Identity { device: std::process::id() as u64, name, os };
            let kind = match source {
                audio::Source::System => aon_core::protocol::Source::System,
                audio::Source::Mic => aon_core::protocol::Source::Mic,
            };
            let cfg = net::SendConfig { hub, who, source: kind, drop_percent, verbose: true };
            net::run_sender(cfg, rx, rate, &net::SendShared::default())
        }
    }
}

/// An engine for CLI testing: random identity, no persisted state.
fn cli_engine(output: Option<String>) -> std::sync::Arc<aon::engine::Engine> {
    use std::hash::{BuildHasher, RandomState};
    let id = RandomState::new().hash_one(std::time::Instant::now()) | 1;
    let name = gethostname::gethostname().to_string_lossy().into_owned();
    let prefs = aon::engine::Prefs { reconnect: false, last_host: None, output, system_source: None, mic: None };
    aon::engine::Engine::new(id, name, prefs)
}

fn resolve(hub: &str) -> Result<SocketAddr> {
    let with_port = if hub.contains(':') { hub.to_string() } else { format!("{hub}:{DEFAULT_PORT}") };
    with_port.to_socket_addrs()?.next().with_context(|| format!("cannot resolve {hub}"))
}
