//! Windows system-audio capture that leaves out Aonin's own playback.
//!
//! Plain WASAPI loopback records everything the endpoint plays, including
//! Aonin's own output, which would be sent straight back to the host. Process
//! loopback (Windows 10 2004+) in "exclude target process tree" mode records
//! every process except this one.

use anyhow::{Result, anyhow};
use rtrb::{Consumer, Producer, RingBuffer};
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::{Arc, mpsc};
use std::thread::JoinHandle;
use std::time::Duration;
use windows::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0};
use windows::Win32::Media::Audio::*;
use windows::Win32::System::Com::StructuredStorage::PROPVARIANT;
use windows::Win32::System::Com::{BLOB, COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize, IAgileObject, IAgileObject_Impl};
use windows::Win32::System::Threading::{CreateEventW, GetCurrentProcessId, WaitForSingleObject};
use windows::Win32::System::Variant::VT_BLOB;
use windows::core::{HRESULT, IUnknown, Interface, Ref, implement};

/// Process loopback converts to whatever format we ask for.
pub const RATE: u32 = 48_000;
const WAVE_FORMAT_IEEE_FLOAT: u16 = 3;

#[implement(IActivateAudioInterfaceCompletionHandler, IAgileObject)]
struct Activated(mpsc::Sender<()>);

impl IActivateAudioInterfaceCompletionHandler_Impl for Activated_Impl {
    fn ActivateCompleted(&self, _op: Ref<IActivateAudioInterfaceAsyncOperation>) -> windows::core::Result<()> {
        let _ = self.0.send(());
        Ok(())
    }
}

impl IAgileObject_Impl for Activated_Impl {}

/// Keeps the capture thread alive; dropping it stops the capture.
pub struct Capture {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Drop for Capture {
    fn drop(&mut self) {
        self.stop.store(true, Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// Start capturing every process except this one, as interleaved stereo f32
/// at `RATE`.
pub fn start() -> Result<(Capture, Consumer<f32>, u32)> {
    let (tx, rx) = RingBuffer::new(RATE as usize * 2);
    let stop = Arc::new(AtomicBool::new(false));
    let (ready_tx, ready_rx) = mpsc::channel::<Result<()>>();
    let s = stop.clone();
    let thread = std::thread::Builder::new().name("aonin-winloop".into()).spawn(move || {
        // Safety: COM calls on a thread that initialises COM itself.
        if let Err(e) = unsafe { run(tx, &s, &ready_tx) } {
            let _ = ready_tx.send(Err(e));
        }
    })?;
    match ready_rx.recv() {
        Ok(Ok(())) => Ok((Capture { stop, thread: Some(thread) }, rx, RATE)),
        Ok(Err(e)) => {
            let _ = thread.join();
            Err(e)
        }
        Err(_) => Err(anyhow!("process loopback thread exited")),
    }
}

unsafe fn run(mut tx: Producer<f32>, stop: &AtomicBool, ready: &mpsc::Sender<Result<()>>) -> Result<()> {
    unsafe {
        CoInitializeEx(None, COINIT_MULTITHREADED).ok()?;
        let result = capture(&mut tx, stop, ready);
        CoUninitialize();
        result
    }
}

unsafe fn capture(tx: &mut Producer<f32>, stop: &AtomicBool, ready: &mpsc::Sender<Result<()>>) -> Result<()> {
    unsafe {
        let params = AUDIOCLIENT_ACTIVATION_PARAMS {
            ActivationType: AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK,
            Anonymous: AUDIOCLIENT_ACTIVATION_PARAMS_0 {
                ProcessLoopbackParams: AUDIOCLIENT_PROCESS_LOOPBACK_PARAMS {
                    TargetProcessId: GetCurrentProcessId(),
                    ProcessLoopbackMode: PROCESS_LOOPBACK_MODE_EXCLUDE_TARGET_PROCESS_TREE,
                },
            },
        };
        // A VT_BLOB pointing at `params`, which outlives the activation.
        let mut pv = PROPVARIANT::default();
        {
            let inner = &mut *pv.Anonymous.Anonymous;
            inner.vt = VT_BLOB;
            inner.Anonymous.blob = BLOB {
                cbSize: size_of::<AUDIOCLIENT_ACTIVATION_PARAMS>() as u32,
                pBlobData: &params as *const _ as *mut u8,
            };
        }

        let (done_tx, done_rx) = mpsc::channel();
        let handler: IActivateAudioInterfaceCompletionHandler = Activated(done_tx).into();
        let op = ActivateAudioInterfaceAsync(VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK, &IAudioClient::IID, Some(&pv), &handler)?;
        done_rx.recv_timeout(Duration::from_secs(5)).map_err(|_| anyhow!("process loopback activation timed out"))?;
        let mut hr = HRESULT(0);
        let mut unknown: Option<IUnknown> = None;
        op.GetActivateResult(&mut hr, &mut unknown)?;
        hr.ok()?;
        let client: IAudioClient = unknown.ok_or_else(|| anyhow!("no audio client"))?.cast()?;

        let format = WAVEFORMATEX {
            wFormatTag: WAVE_FORMAT_IEEE_FLOAT,
            nChannels: 2,
            nSamplesPerSec: RATE,
            nAvgBytesPerSec: RATE * 8,
            nBlockAlign: 8,
            wBitsPerSample: 32,
            cbSize: 0,
        };
        client.Initialize(
            AUDCLNT_SHAREMODE_SHARED,
            AUDCLNT_STREAMFLAGS_LOOPBACK | AUDCLNT_STREAMFLAGS_EVENTCALLBACK | AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM,
            200_000, // 20 ms, in 100 ns units
            0,
            &format,
            None,
        )?;
        let event = CreateEventW(None, false, false, None)?;
        client.SetEventHandle(event)?;
        let reader: IAudioCaptureClient = client.GetService()?;
        client.Start()?;
        let _ = ready.send(Ok(()));

        let mut result = Ok(());
        'run: while !stop.load(Relaxed) {
            if WaitForSingleObject(event, 100) != WAIT_OBJECT_0 {
                continue;
            }
            loop {
                let pending = match reader.GetNextPacketSize() {
                    Ok(n) => n,
                    Err(e) => {
                        result = Err(e.into());
                        break 'run;
                    }
                };
                if pending == 0 {
                    break;
                }
                let mut data = std::ptr::null_mut();
                let mut frames = 0u32;
                let mut flags = 0u32;
                if let Err(e) = reader.GetBuffer(&mut data, &mut frames, &mut flags, None, None) {
                    result = Err(e.into());
                    break 'run;
                }
                let n = frames as usize * 2;
                if tx.slots() >= n {
                    if flags & AUDCLNT_BUFFERFLAGS_SILENT.0 as u32 != 0 || data.is_null() {
                        for _ in 0..n {
                            let _ = tx.push(0.0);
                        }
                    } else {
                        for &s in std::slice::from_raw_parts(data as *const f32, n) {
                            let _ = tx.push(s);
                        }
                    }
                }
                let _ = reader.ReleaseBuffer(frames);
            }
        }
        let _ = client.Stop();
        let _ = CloseHandle(event);
        result
    }
}
