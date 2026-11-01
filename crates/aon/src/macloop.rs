//! macOS system-audio capture that leaves out Aonin's own playback.
//!
//! A Core Audio process tap (macOS 14.2+) on the chosen output device,
//! marked exclusive with Aonin's own process in its list, records every app
//! except this one. The tap lives in a private aggregate device, which cpal
//! then opens as an ordinary input device. Unlike a plain input stream, this
//! works the same when the output is a virtual loopback device such as
//! BlackHole.
//!
//! The tap and aggregate-device setup follows cpal's
//! `host/coreaudio/macos/loopback.rs` (Apache-2.0, RustAudio contributors),
//! with the process exclusion added.

use anyhow::{Result, anyhow};
use cpal::traits::HostTrait;
use objc2::AnyThread;
use objc2::rc::Retained;
use objc2_core_audio::{
    AudioHardwareCreateAggregateDevice, AudioHardwareCreateProcessTap, AudioHardwareDestroyAggregateDevice,
    AudioHardwareDestroyProcessTap, AudioObjectGetPropertyData, AudioObjectID, AudioObjectPropertyAddress, CATapDescription,
    CATapMuteBehavior, kAudioAggregateDeviceNameKey, kAudioAggregateDeviceTapAutoStartKey, kAudioAggregateDeviceTapListKey,
    kAudioAggregateDeviceUIDKey, kAudioEndPointDeviceIsPrivateKey, kAudioHardwarePropertyTranslatePIDToProcessObject,
    kAudioObjectPropertyElementMain, kAudioObjectPropertyScopeGlobal, kAudioObjectSystemObject,
    kAudioSubTapDriftCompensationKey, kAudioSubTapUIDKey,
};
use objc2_core_foundation::{
    CFArray, CFDictionary, CFMutableDictionary, CFRetained, CFString, kCFAllocatorDefault, kCFTypeArrayCallBacks,
    kCFTypeDictionaryKeyCallBacks, kCFTypeDictionaryValueCallBacks,
};
use objc2_foundation::{NSArray, NSNumber, NSString};
use std::ffi::{CStr, c_void};
use std::ptr::NonNull;
use std::sync::atomic::{AtomicU32, Ordering::Relaxed};

static INSTANCE: AtomicU32 = AtomicU32::new(0);

/// Keeps the tap and its aggregate device alive; drop it after the stream
/// reading from the device.
pub struct Tap {
    tap: AudioObjectID,
    device: AudioObjectID,
}

impl Drop for Tap {
    fn drop(&mut self) {
        // Safety: both objects were created by `start` and are destroyed once.
        unsafe {
            AudioHardwareDestroyAggregateDevice(self.device);
            AudioHardwareDestroyProcessTap(self.tap);
        }
    }
}

fn check(status: i32, what: &str) -> Result<()> {
    if status == 0 { Ok(()) } else { Err(anyhow!("{what} failed (OSStatus {status})")) }
}

/// Core Audio's process object for this process, so the tap can exclude it.
fn own_process_object() -> Option<AudioObjectID> {
    let pid = std::process::id() as i32;
    let address = AudioObjectPropertyAddress {
        mSelector: kAudioHardwarePropertyTranslatePIDToProcessObject,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMain,
    };
    let mut object: AudioObjectID = 0;
    let mut size = size_of::<AudioObjectID>() as u32;
    // Safety: the property takes a pid_t qualifier and writes one AudioObjectID.
    let status = unsafe {
        AudioObjectGetPropertyData(
            kAudioObjectSystemObject as AudioObjectID,
            NonNull::from(&address),
            size_of::<i32>() as u32,
            &pid as *const i32 as *const c_void,
            NonNull::from(&mut size),
            NonNull::from(&mut object).cast(),
        )
    };
    (status == 0 && object != 0).then_some(object)
}

fn cfstr(key: &'static CStr) -> CFRetained<CFString> {
    // Safety: a NUL-terminated UTF-8 constant from the Core Audio headers.
    unsafe { CFString::with_c_string(kCFAllocatorDefault, key.as_ptr(), 0x0800_0100) }.expect("CFString")
}

/// The private aggregate device holding just our tap.
fn aggregate_properties(tap_uid: &NSString, uid: &str, name: &str) -> CFRetained<CFDictionary> {
    let yes = NSNumber::initWithBool(NSNumber::alloc(), true);
    // Safety: CF containers built with the standard CF type callbacks; every
    // key and value is a live CF/NS object for the duration of the call.
    unsafe {
        let tap = CFMutableDictionary::new(kCFAllocatorDefault, 2, &kCFTypeDictionaryKeyCallBacks, &kCFTypeDictionaryValueCallBacks).unwrap();
        CFMutableDictionary::set_value(Some(&tap), &*cfstr(kAudioSubTapUIDKey) as *const _ as *const c_void, tap_uid as *const _ as *const c_void);
        CFMutableDictionary::set_value(
            Some(&tap),
            &*cfstr(kAudioSubTapDriftCompensationKey) as *const _ as *const c_void,
            &*yes as *const _ as *const c_void,
        );
        let taps_list = [tap];
        let taps = CFArray::new(kCFAllocatorDefault, taps_list.as_ptr() as *mut *const c_void, 1, &kCFTypeArrayCallBacks).unwrap();

        let dict = CFMutableDictionary::new(kCFAllocatorDefault, 5, &kCFTypeDictionaryKeyCallBacks, &kCFTypeDictionaryValueCallBacks).unwrap();
        let set = |k: &'static CStr, v: *const c_void| CFMutableDictionary::set_value(Some(&dict), &*cfstr(k) as *const _ as *const c_void, v);
        set(kAudioAggregateDeviceNameKey, &*CFString::from_str(name) as *const _ as *const c_void);
        set(kAudioAggregateDeviceUIDKey, &*CFString::from_str(uid) as *const _ as *const c_void);
        set(kAudioAggregateDeviceTapListKey, &*taps as *const _ as *const c_void);
        set(kAudioAggregateDeviceTapAutoStartKey, &*yes as *const _ as *const c_void);
        set(kAudioEndPointDeviceIsPrivateKey, &*yes as *const _ as *const c_void);
        CFRetained::cast_unchecked::<CFDictionary>(dict)
    }
}

/// Tap the output of the device with this Core Audio UID, minus Aonin's own
/// audio. With `mute`, the device goes silent while the tap is read (Aonin's
/// own playback still plays). Returns the tap guard and the cpal input device
/// that reads it. Core Audio removes the tap if Aonin dies, so a crash never
/// leaves the speakers muted.
pub fn start(device_uid: &str, mute: bool) -> Result<(Tap, cpal::Device)> {
    let me = own_process_object().ok_or_else(|| anyhow!("Core Audio has no process object for Aonin"))?;
    let n = INSTANCE.fetch_add(1, Relaxed);
    let pid = std::process::id();
    let uid = format!("io.github.rosydaqbar.aonin.capture.{pid}.{n}");

    // Safety: plain Core Audio / Objective-C calls with live arguments; every
    // object created here is destroyed by `Tap::drop`.
    let guard = unsafe {
        let processes: Retained<NSArray<NSNumber>> = NSArray::from_retained_slice(&[NSNumber::new_u32(me)]);
        let desc = CATapDescription::initWithProcesses_andDeviceUID_withStream(
            CATapDescription::alloc(),
            &processes,
            &NSString::from_str(device_uid),
            0,
        );
        desc.setMuteBehavior(if mute { CATapMuteBehavior::MutedWhenTapped } else { CATapMuteBehavior::Unmuted });
        desc.setName(&NSString::from_str(&format!("Aonin capture {pid}.{n}")));
        desc.setPrivate(true);
        desc.setExclusive(true); // the process list is what gets left out

        let mut tap: AudioObjectID = 0;
        check(AudioHardwareCreateProcessTap(Some(&desc), &mut tap), "creating the process tap")?;
        let tap_uid = desc.UUID().UUIDString();

        let props = aggregate_properties(&tap_uid, &uid, "Aonin capture");
        let mut device: AudioObjectID = 0;
        if let Err(e) = check(AudioHardwareCreateAggregateDevice(&props, NonNull::from(&mut device)), "creating the capture device") {
            AudioHardwareDestroyProcessTap(tap);
            return Err(e);
        }
        Tap { tap, device }
    };

    // The new device can take a moment to show up in the device list.
    let host = cpal::default_host();
    let id = cpal::DeviceId::new(host.id(), &uid);
    for _ in 0..50 {
        if let Some(device) = host.device_by_id(&id) {
            return Ok((guard, device));
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    Err(anyhow!("the capture device never appeared"))
}
