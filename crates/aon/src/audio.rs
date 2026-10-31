//! Device I/O through cpal: system-audio loopback or mic capture, and the
//! mixing output stream on the hub.

use anyhow::{Context, Result, anyhow, bail};
use aon_core::playout::Playout;
use aon_core::resample::Frame;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, FromSample, SampleFormat, SizedSample, Stream, StreamConfig};
use crossbeam_channel::{Receiver, Sender};
use rtrb::{Consumer, Producer, RingBuffer};

#[derive(Clone, Copy, Debug, clap::ValueEnum)]
pub enum Source {
    /// Everything the machine is playing (loopback of the output device).
    System,
    /// A microphone / line input.
    Mic,
}

pub fn list_devices() -> Result<()> {
    let host = cpal::default_host();
    let def_out = host.default_output_device().map(|d| d.to_string());
    let def_in = host.default_input_device().map(|d| d.to_string());
    println!("Output devices (usable for --source system and as hub output):");
    for d in host.output_devices()? {
        let name = d.to_string();
        let mark = if Some(&name) == def_out.as_ref() { " (default)" } else { "" };
        println!("  {name}{mark}");
    }
    println!("Input devices (usable for --source mic):");
    for d in host.input_devices()? {
        let name = d.to_string();
        let mark = if Some(&name) == def_in.as_ref() { " (default)" } else { "" };
        println!("  {name}{mark}");
    }
    Ok(())
}

fn pick(devices: impl Iterator<Item = Device>, default: Option<Device>, name: Option<&str>) -> Result<Device> {
    match name {
        None => default.ok_or_else(|| anyhow!("no default device")),
        Some(want) => {
            let want = want.to_lowercase();
            devices
                .into_iter()
                .find(|d| d.to_string().to_lowercase().contains(&want))
                .ok_or_else(|| anyhow!("no device matching {want:?} (see `aon devices`)"))
        }
    }
}

/// Start capturing. Returns the stream (keep it alive), a ring of interleaved
/// stereo f32 samples, and the device sample rate.
pub fn start_capture(source: Source, name: Option<&str>) -> Result<(Stream, Consumer<f32>, u32)> {
    let host = cpal::default_host();
    let (device, supported) = match source {
        Source::System => {
            let d = pick(host.output_devices()?, host.default_output_device(), name)?;
            let c = d.default_output_config()?;
            (d, c)
        }
        Source::Mic => {
            let d = pick(host.input_devices()?, host.default_input_device(), name)?;
            let c = d.default_input_config()?;
            (d, c)
        }
    };
    let config = supported.config();
    let rate = config.sample_rate;
    println!("Capturing {source:?} from \"{device}\" ({} ch, {rate} Hz, {:?})", config.channels, supported.sample_format());

    // 1 s of headroom; the sender drains it every ~1 ms.
    let (tx, rx) = RingBuffer::new(rate as usize * 2);
    let stream = match supported.sample_format() {
        SampleFormat::I16 => build_capture::<i16>(&device, config, tx),
        SampleFormat::I32 => build_capture::<i32>(&device, config, tx),
        SampleFormat::F32 => build_capture::<f32>(&device, config, tx),
        SampleFormat::F64 => build_capture::<f64>(&device, config, tx),
        SampleFormat::U16 => build_capture::<u16>(&device, config, tx),
        SampleFormat::U8 => build_capture::<u8>(&device, config, tx),
        f => bail!("unsupported sample format {f:?}"),
    }?;
    stream.play().context("starting capture")?;
    Ok((stream, rx, rate))
}

fn build_capture<T>(device: &Device, config: StreamConfig, mut tx: Producer<f32>) -> Result<Stream>
where
    T: SizedSample,
    f32: FromSample<T>,
{
    let ch = config.channels as usize;
    let stream = device.build_input_stream(
        config,
        move |data: &[T], _| {
            for f in data.chunks_exact(ch) {
                let l = f[0].to_sample::<f32>();
                let r = if ch > 1 { f[1].to_sample::<f32>() } else { l };
                if tx.slots() >= 2 {
                    let _ = tx.push(l);
                    let _ = tx.push(r);
                }
            }
        },
        |e| eprintln!("capture error: {e}"),
        None,
    )?;
    Ok(stream)
}

/// Start the hub's output. New incoming streams are handed to the audio
/// callback through the returned channel.
pub fn start_output(name: Option<&str>) -> Result<(Stream, Sender<Playout>, u32)> {
    let host = cpal::default_host();
    let device = pick(host.output_devices()?, host.default_output_device(), name)?;
    let supported = device.default_output_config()?;
    let config = supported.config();
    let rate = config.sample_rate;
    println!("Playing to \"{device}\" ({} ch, {rate} Hz, {:?})", config.channels, supported.sample_format());

    let (tx, rx) = crossbeam_channel::bounded(64);
    let stream = match supported.sample_format() {
        SampleFormat::I16 => build_output::<i16>(&device, config, rx),
        SampleFormat::I32 => build_output::<i32>(&device, config, rx),
        SampleFormat::F32 => build_output::<f32>(&device, config, rx),
        SampleFormat::F64 => build_output::<f64>(&device, config, rx),
        SampleFormat::U16 => build_output::<u16>(&device, config, rx),
        SampleFormat::U8 => build_output::<u8>(&device, config, rx),
        f => bail!("unsupported sample format {f:?}"),
    }?;
    stream.play().context("starting output")?;
    Ok((stream, tx, rate))
}

fn build_output<T>(device: &Device, config: StreamConfig, rx: Receiver<Playout>) -> Result<Stream>
where
    T: SizedSample + FromSample<f32>,
{
    let ch = config.channels as usize;
    let mut playouts: Vec<Playout> = Vec::with_capacity(32);
    let mut mix: Vec<Frame> = vec![[0.0; 2]; 8192];
    let stream = device.build_output_stream(
        config,
        move |data: &mut [T], _| {
            while let Ok(p) = rx.try_recv() {
                playouts.push(p);
            }
            let frames = data.len() / ch;
            if mix.len() < frames {
                mix.resize(frames, [0.0; 2]);
            }
            let mix = &mut mix[..frames];
            mix.fill([0.0; 2]);
            for p in playouts.iter_mut() {
                p.render_add(mix);
            }
            playouts.retain(|p| !p.finished());
            for (out, m) in data.chunks_exact_mut(ch).zip(mix.iter()) {
                if ch == 1 {
                    out[0] = T::from_sample((m[0] + m[1]) * 0.5);
                } else {
                    out[0] = T::from_sample(m[0].clamp(-1.0, 1.0));
                    out[1] = T::from_sample(m[1].clamp(-1.0, 1.0));
                }
            }
        },
        |e| eprintln!("output error: {e}"),
        None,
    )?;
    Ok(stream)
}

/// Play a sine tone on an output device until the process exits. Test aid.
pub fn play_tone(name: Option<&str>, freq: f32, level: f32) -> Result<Stream> {
    let host = cpal::default_host();
    let device = pick(host.output_devices()?, host.default_output_device(), name)?;
    let config: StreamConfig = device.default_output_config()?.config();
    let (ch, rate) = (config.channels as usize, config.sample_rate as f32);
    println!("Tone {freq} Hz at {:.0} dBFS on \"{device}\"", 20.0 * level.log10());
    let mut phase = 0.0f32;
    let stream = device.build_output_stream(
        config,
        move |data: &mut [f32], _| {
            for f in data.chunks_exact_mut(ch) {
                let s = (phase * std::f32::consts::TAU).sin() * level;
                phase = (phase + freq / rate).fract();
                f[0] = s;
                if ch > 1 {
                    f[1] = s;
                }
            }
        },
        |e| eprintln!("tone error: {e}"),
        None,
    )?;
    stream.play()?;
    Ok(stream)
}

/// Names of the output and input devices, and the current defaults.
pub struct DeviceNames {
    pub outputs: Vec<String>,
    pub inputs: Vec<String>,
    pub default_output: Option<String>,
    pub default_input: Option<String>,
}

pub fn device_names() -> DeviceNames {
    let host = cpal::default_host();
    DeviceNames {
        outputs: host.output_devices().map(|ds| ds.map(|d| d.to_string()).collect()).unwrap_or_default(),
        inputs: host.input_devices().map(|ds| ds.map(|d| d.to_string()).collect()).unwrap_or_default(),
        default_output: host.default_output_device().map(|d| d.to_string()),
        default_input: host.default_input_device().map(|d| d.to_string()),
    }
}
