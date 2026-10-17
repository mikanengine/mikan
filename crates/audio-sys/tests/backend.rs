//! Tests for the C device backend: the offline ring, its conversion, its
//! overrun accounting, and the device kind's documented outcomes on a
//! machine with or without audio hardware.

use std::ffi::c_void;
use std::ptr;
use std::sync::atomic::{AtomicUsize, Ordering};

use mikan_audio_sys::reference::to_s16;
use mikan_audio_sys::{AudioError, Backend, BackendError, BackendKind, System};

static CALLBACK_CALLS: AtomicUsize = AtomicUsize::new(0);

/// Fills every frame with +0.25 and counts the call.
unsafe extern "C" fn flat_quarter(_user: *mut c_void, frames: *mut f32, frame_count: u32) {
    CALLBACK_CALLS.fetch_add(1, Ordering::SeqCst);
    for i in 0..(frame_count as usize * 2) {
        // SAFETY: the backend guarantees the buffer holds
        // `frame_count * CHANNELS` samples.
        unsafe { frames.add(i).write(0.25) };
    }
}

/// Fills every frame with silence.
unsafe extern "C" fn silence(_user: *mut c_void, frames: *mut f32, frame_count: u32) {
    for i in 0..(frame_count as usize * 2) {
        // SAFETY: as `flat_quarter`.
        unsafe { frames.add(i).write(0.0) };
    }
}

#[test]
fn offline_pump_invokes_the_render_callback_and_converts() {
    CALLBACK_CALLS.store(0, Ordering::SeqCst);
    // SAFETY: the callback never touches the module and the user pointer is
    // null; the backend is destroyed before the function returns.
    let mut backend = unsafe {
        Backend::create(
            BackendKind::Offline,
            48_000,
            4,
            flat_quarter,
            ptr::null_mut(),
        )
    }
    .expect("the offline backend does not depend on hardware");

    let info = backend.info();
    assert_eq!(info.kind, BackendKind::Offline);
    assert_eq!(info.sample_rate, 48_000);
    assert_eq!(info.channels, 2);
    assert_eq!(info.backend_name, "offline");
    assert_eq!(info.device_name, "offline");
    assert!(!backend.is_running());
    assert_eq!(backend.device_events().reroutes, 0);

    backend.pump(4).expect("a whole block pumps");
    assert_eq!(CALLBACK_CALLS.load(Ordering::SeqCst), 1);

    let mut out = [0_i16; 8];
    let read = backend.read_s16(&mut out).expect("the ring drains");
    assert_eq!(read, 4);
    let expected = to_s16(0.25);
    assert!(
        out.iter().all(|&sample| sample == expected),
        "every sample quantizes {expected}, got {out:?}"
    );
    assert_eq!(backend.overruns(), 0);
}

#[test]
fn offline_pump_validates_its_frame_count() {
    // SAFETY: as above.
    let mut backend =
        unsafe { Backend::create(BackendKind::Offline, 48_000, 8, silence, ptr::null_mut()) }
            .expect("offline backend");
    assert_eq!(backend.pump(0).unwrap_err(), BackendError::BadArgument);
    assert_eq!(backend.pump(9).unwrap_err(), BackendError::BadArgument);
    backend.pump(8).expect("exactly the block size is fine");
    // The ring is idle; a drain with nothing buffered reads zero frames.
    let read = backend.read_s16(&mut [0_i16; 2]).expect("read");
    assert_eq!(read, 1);
    backend.read_s16(&mut []).expect("an empty drain is legal");

    // Starting and stopping only flips a status flag for the offline kind.
    backend.start().expect("start");
    assert!(backend.is_running());
    backend.stop().expect("stop");
    assert!(!backend.is_running());
}

#[test]
fn offline_overruns_are_dropped_and_counted() {
    // Block of 8 frames, ring of `RING_BLOCKS * block` = 64 frames: fill it
    // without draining and the next block is dropped.
    // SAFETY: as above.
    let mut backend = unsafe {
        Backend::create(
            BackendKind::Offline,
            48_000,
            8,
            flat_quarter,
            ptr::null_mut(),
        )
    }
    .expect("offline backend");
    for _ in 0..8 {
        backend.pump(8).expect("pump");
    }
    assert_eq!(backend.overruns(), 0, "64 buffered frames fit the ring");
    backend.pump(8).expect("a dropped block is not an error");
    assert_eq!(backend.overruns(), 1);

    // The retained samples are the oldest ones and drain in order.
    let mut out = [0_i16; 128];
    let first = backend.read_s16(&mut out[..8]).expect("read");
    assert_eq!(first, 4);
    let rest = backend.read_s16(&mut out).expect("read");
    assert_eq!(rest, 60);
    assert_eq!(backend.overruns(), 1);
}

#[test]
fn offline_read_pressure_on_a_device_kind_is_a_bad_argument() {
    // The offline-only entry points must refuse a device backend, device
    // or not; this machine may have no device at all.
    match unsafe { Backend::create(BackendKind::Device, 48_000, 64, silence, ptr::null_mut()) } {
        Ok(mut backend) => {
            assert_eq!(backend.info().kind, BackendKind::Device);
            assert_eq!(backend.pump(64).unwrap_err(), BackendError::BadArgument);
            assert_eq!(
                backend.read_s16(&mut [0_i16; 8]).unwrap_err(),
                BackendError::BadArgument
            );
        }
        Err(BackendError::NoDevice) => { /* documented outcome without hardware */ }
        Err(other) => panic!("unexpected device error: {other:?}"),
    }
}

#[test]
fn device_kind_opens_or_reports_no_device() {
    match unsafe { Backend::create(BackendKind::Device, 48_000, 256, silence, ptr::null_mut()) } {
        Ok(mut backend) => {
            assert_eq!(backend.info().kind, BackendKind::Device);
            assert!(!backend.info().backend_name.is_empty());
            assert!(!backend.info().device_name.is_empty());
            backend.start().expect("the opened device starts");
            assert!(backend.is_running());
            backend.stop().expect("the started device stops");
            assert!(!backend.is_running());
        }
        Err(BackendError::NoDevice) => {
            // This machine (or CI) has no usable audio hardware: the
            // documented outcome, never a silent null device.
        }
        Err(other) => panic!("unexpected device error: {other:?}"),
    }
}

#[test]
fn system_offline_wires_both_layers() {
    let mut system = System::new_offline(48_000, 32).expect("offline system");
    assert_eq!(system.info().kind, BackendKind::Offline);
    assert_eq!(system.runtime().sample_rate(), 48_000);
    assert_eq!(system.pump(33).unwrap_err(), AudioError::BadArgument);
    assert_eq!(system.pump(0).unwrap_err(), AudioError::BadArgument);
    system.pump(32).expect("pump");
    let mut out = [0_i16; 64];
    assert_eq!(system.read_s16(&mut out).expect("read"), 32);
}
