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

fn resolve(hub: &str) -> Result<SocketAddr> {
    let with_port = if hub.contains(':') { hub.to_string() } else { format!("{hub}:{DEFAULT_PORT}") };
    with_port.to_socket_addrs()?.next().with_context(|| format!("cannot resolve {hub}"))
}
