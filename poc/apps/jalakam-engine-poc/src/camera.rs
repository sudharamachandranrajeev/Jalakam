//! Webcam capture with the Media Foundation source reader (ADR-0007).
//!
//! A dedicated thread reads samples synchronously and publishes the newest
//! frame into a latest-wins mailbox. The render thread uploads it. Errors map
//! to stable codes; a lost device is retried every 2 s.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

use anyhow::Result;
use jkp_proto::State;
use parking_lot::Mutex;
use windows::core::{GUID, HRESULT, PWSTR};
use windows::Win32::Foundation::E_ACCESSDENIED;
use windows::Win32::Media::MediaFoundation::*;
use windows::Win32::System::Com::{CoInitializeEx, CoTaskMemFree, CoUninitialize, COINIT_MULTITHREADED};

use crate::status::StatusCell;

const FIRST_VIDEO_STREAM: u32 = 0xFFFF_FFFC; // MF_SOURCE_READER_FIRST_VIDEO_STREAM

pub struct CamFrame {
    pub width: u32,
    pub height: u32,
    /// Tightly packed BGRX rows, top-down.
    pub data: Vec<u8>,
    pub seq: u64,
}

#[derive(Default)]
pub struct CameraShared {
    pub status: StatusCell,
    pub latest: Mutex<Option<Arc<CamFrame>>>,
    pub frames: AtomicU64,
    pub name: Mutex<String>,
    pub format: Mutex<String>,
    stop: AtomicBool,
    source: Mutex<Option<IMFMediaSource>>,
}

// COM pointers inside are only used under the mutex; MF objects are free-threaded.
unsafe impl Send for CameraShared {}
unsafe impl Sync for CameraShared {}

pub struct Camera {
    pub shared: Arc<CameraShared>,
    thread: Option<JoinHandle<()>>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct CameraInfo {
    pub index: usize,
    pub name: String,
    pub symbolic_link: String,
    pub formats: Vec<String>,
    pub error: Option<String>,
}

struct Device {
    activate: IMFActivate,
    name: String,
    link: String,
}

fn alloc_string(a: &IMFActivate, key: &GUID) -> String {
    unsafe {
        let mut p = PWSTR::null();
        let mut len = 0u32;
        if a.GetAllocatedString(key, &mut p, &mut len).is_ok() {
            let s = p.to_string().unwrap_or_default();
            CoTaskMemFree(Some(p.0 as *const _));
            s
        } else {
            String::new()
        }
    }
}

fn enum_devices() -> Result<Vec<Device>> {
    unsafe {
        let mut attr = None;
        MFCreateAttributes(&mut attr, 1)?;
        let attr: IMFAttributes = attr.unwrap();
        attr.SetGUID(&MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE, &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_GUID)?;
        let mut pp: *mut Option<IMFActivate> = std::ptr::null_mut();
        let mut count = 0u32;
        MFEnumDeviceSources(&attr, &mut pp, &mut count)?;
        let mut out = Vec::new();
        for i in 0..count as usize {
            if let Some(a) = (*pp.add(i)).take() {
                let name = alloc_string(&a, &MF_DEVSOURCE_ATTRIBUTE_FRIENDLY_NAME);
                let link = alloc_string(&a, &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_SYMBOLIC_LINK);
                out.push(Device { activate: a, name, link });
            }
        }
        if !pp.is_null() {
            CoTaskMemFree(Some(pp as *const _));
        }
        Ok(out)
    }
}

fn subtype_name(g: &GUID) -> String {
    let known = [
        (MFVideoFormat_NV12, "NV12"),
        (MFVideoFormat_YUY2, "YUY2"),
        (MFVideoFormat_MJPG, "MJPG"),
        (MFVideoFormat_RGB32, "RGB32"),
        (MFVideoFormat_RGB24, "RGB24"),
        (MFVideoFormat_I420, "I420"),
        (MFVideoFormat_H264, "H264"),
        (MFVideoFormat_UYVY, "UYVY"),
    ];
    known.iter().find(|(k, _)| k == g).map(|(_, n)| n.to_string()).unwrap_or_else(|| format!("{:?}", g))
}

#[derive(Clone)]
struct NativeType {
    mt: IMFMediaType,
    subtype: GUID,
    w: u32,
    h: u32,
    fps: f64,
}

fn native_types(reader: &IMFSourceReader) -> Vec<NativeType> {
    let mut v = Vec::new();
    for i in 0.. {
        let mt = match unsafe { reader.GetNativeMediaType(FIRST_VIDEO_STREAM, i) } {
            Ok(m) => m,
            Err(_) => break,
        };
        unsafe {
            let subtype = mt.GetGUID(&MF_MT_SUBTYPE).unwrap_or_default();
            let size = mt.GetUINT64(&MF_MT_FRAME_SIZE).unwrap_or(0);
            let rate = mt.GetUINT64(&MF_MT_FRAME_RATE).unwrap_or(0);
            let (num, den) = ((rate >> 32) as u32, (rate & 0xffff_ffff) as u32);
            v.push(NativeType {
                mt,
                subtype,
                w: (size >> 32) as u32,
                h: (size & 0xffff_ffff) as u32,
                fps: if den == 0 { 0.0 } else { num as f64 / den as f64 },
            });
        }
    }
    v
}

fn open_reader(dev: &Device) -> windows::core::Result<(IMFMediaSource, IMFSourceReader)> {
    unsafe {
        let source: IMFMediaSource = dev.activate.ActivateObject()?;
        let mut attr = None;
        MFCreateAttributes(&mut attr, 2)?;
        let attr: IMFAttributes = attr.unwrap();
        attr.SetUINT32(&MF_SOURCE_READER_ENABLE_ADVANCED_VIDEO_PROCESSING, 1)?;
        let reader = MFCreateSourceReaderFromMediaSource(&source, &attr)?;
        Ok((source, reader))
    }
}

pub fn list_cameras() -> Vec<CameraInfo> {
    let devs = match enum_devices() {
        Ok(d) => d,
        Err(e) => {
            crate::log!("camera enumeration failed: {e}");
            return Vec::new();
        }
    };
    devs.iter()
        .enumerate()
        .map(|(index, d)| {
            let (formats, error) = match open_reader(d) {
                Ok((source, reader)) => {
                    let mut f: Vec<String> = native_types(&reader)
                        .iter()
                        .map(|t| format!("{}x{}@{:.0} {}", t.w, t.h, t.fps, subtype_name(&t.subtype)))
                        .collect();
                    f.dedup();
                    unsafe {
                        let _ = source.Shutdown();
                    }
                    (f, None)
                }
                Err(e) => (Vec::new(), Some(map_hr(e.code()).to_string() + ": " + &e.message())),
            };
            CameraInfo { index, name: d.name.clone(), symbolic_link: d.link.clone(), formats, error }
        })
        .collect()
}

fn map_hr(hr: HRESULT) -> &'static str {
    if hr == E_ACCESSDENIED {
        "CAP.CAMERA.ACCESS_DENIED"
    } else if hr == MF_E_VIDEO_RECORDING_DEVICE_INVALIDATED {
        "CAP.CAMERA.DISCONNECTED"
    } else if hr == MF_E_VIDEO_RECORDING_DEVICE_PREEMPTED || hr == MF_E_HW_MFT_FAILED_START_STREAMING {
        "CAP.CAMERA.IN_USE"
    } else {
        "CAP.CAMERA.READ_FAILED"
    }
}

fn select<'a>(devs: &'a [Device], selector: &str) -> Option<&'a Device> {
    if let Ok(i) = selector.parse::<usize>() {
        return devs.get(i);
    }
    let s = selector.to_lowercase();
    devs.iter().find(|d| d.name.to_lowercase().contains(&s) || d.link.eq_ignore_ascii_case(selector))
}

/// Pick a native type: exact size preferred, then highest fps not above the
/// request, uncompressed before MJPEG.
fn pick_type(types: &[NativeType], w: u32, h: u32, fps: u32) -> Option<NativeType> {
    let score = |t: &NativeType| {
        let size = if t.w == w && t.h == h { 0 } else { 1 + ((t.w as i64 * t.h as i64) - (w as i64 * h as i64)).unsigned_abs() / 1000 };
        let rate = if t.fps <= fps as f64 + 0.5 { ((fps as f64 - t.fps) * 10.0) as u64 } else { 10_000 };
        let comp = if t.subtype == MFVideoFormat_MJPG { 1 } else { 0 };
        (size, rate, comp)
    };
    types.iter().min_by_key(|t| score(t)).cloned()
}

impl Camera {
    pub fn start(selector: String, w: u32, h: u32, fps: u32) -> Self {
        let shared = Arc::new(CameraShared::default());
        shared.status.ok(State::Starting);
        let sh = shared.clone();
        let thread = std::thread::Builder::new()
            .name("cam".into())
            .spawn(move || {
                unsafe {
                    let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
                }
                run(&sh, &selector, w, h, fps);
                *sh.source.lock() = None;
                unsafe { CoUninitialize() };
            })
            .expect("spawn cam thread");
        Self { shared, thread: Some(thread) }
    }

    pub fn stop(mut self) {
        self.shared.stop.store(true, Ordering::SeqCst);
        if let Some(t) = self.thread.take() {
            // A live camera returns from ReadSample within one frame interval,
            // and the loop then sees `stop`. Shutting the source down from this
            // thread while a synchronous ReadSample is in flight blocked
            // indefinitely on the test machine, so it is only a last resort
            // for a stalled device.
            let deadline = std::time::Instant::now() + Duration::from_secs(1);
            while !t.is_finished() && std::time::Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(10));
            }
            if !t.is_finished() {
                crate::log!("camera stalled on stop; shutting the source down");
                let src = self.shared.source.lock().clone();
                if let Some(src) = src {
                    unsafe {
                        let _ = src.Shutdown();
                    }
                }
            }
            let _ = t.join();
        }
        *self.shared.latest.lock() = None;
        self.shared.status.ok(State::Idle);
    }
}

fn sleep_unless_stopped(sh: &CameraShared, d: Duration) {
    let step = Duration::from_millis(50);
    let mut t = Duration::ZERO;
    while t < d && !sh.stop.load(Ordering::SeqCst) {
        std::thread::sleep(step);
        t += step;
    }
}

fn run(sh: &CameraShared, selector: &str, w: u32, h: u32, fps: u32) {
    let mut ever_opened = false;
    while !sh.stop.load(Ordering::SeqCst) {
        let devs = match enum_devices() {
            Ok(d) => d,
            Err(e) => {
                sh.status.set(State::Failed, Some("CAP.CAMERA.ENUM_FAILED"), Some(e.to_string()));
                sleep_unless_stopped(sh, Duration::from_secs(2));
                continue;
            }
        };
        let Some(dev) = select(&devs, selector) else {
            let code = if ever_opened { "CAP.CAMERA.DISCONNECTED" } else { "CAP.CAMERA.NOT_FOUND" };
            let state = if ever_opened { State::Degraded } else { State::Failed };
            let names: Vec<_> = devs.iter().map(|d| d.name.as_str()).collect();
            sh.status.set(state, Some(code), Some(format!("selector '{selector}', available: {names:?}")));
            sleep_unless_stopped(sh, Duration::from_secs(2));
            continue;
        };
        *sh.name.lock() = dev.name.clone();
        let (source, reader) = match open_reader(dev) {
            Ok(x) => x,
            Err(e) => {
                sh.status.set(State::Failed, Some(map_hr(e.code())), Some(format!("{}: {}", dev.name, e.message())));
                sleep_unless_stopped(sh, Duration::from_secs(2));
                continue;
            }
        };
        *sh.source.lock() = Some(source.clone());
        match configure(&reader, w, h, fps) {
            Ok(fmt) => *sh.format.lock() = fmt,
            Err(e) => {
                sh.status.set(State::Failed, Some("CAP.CAMERA.UNSUPPORTED_FORMAT"), Some(e.to_string()));
                unsafe {
                    let _ = source.Shutdown();
                }
                *sh.source.lock() = None;
                sleep_unless_stopped(sh, Duration::from_secs(5));
                continue;
            }
        }
        ever_opened = true;
        let err = read_loop(sh, &reader);
        *sh.latest.lock() = None;
        unsafe {
            let _ = source.Shutdown();
        }
        *sh.source.lock() = None;
        if sh.stop.load(Ordering::SeqCst) {
            break;
        }
        if let Some((code, detail)) = err {
            sh.status.set(State::Degraded, Some(code), Some(detail));
        }
        sleep_unless_stopped(sh, Duration::from_secs(2));
    }
}

fn configure(reader: &IMFSourceReader, w: u32, h: u32, fps: u32) -> Result<String> {
    let types = native_types(reader);
    let t = pick_type(&types, w, h, fps).ok_or_else(|| anyhow::anyhow!("device reports no video media types"))?;
    unsafe {
        reader.SetCurrentMediaType(FIRST_VIDEO_STREAM, None, &t.mt)?;
        let out = MFCreateMediaType()?;
        out.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
        out.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_RGB32)?;
        out.SetUINT64(&MF_MT_FRAME_SIZE, ((t.w as u64) << 32) | t.h as u64)?;
        reader
            .SetCurrentMediaType(FIRST_VIDEO_STREAM, None, &out)
            .map_err(|e| anyhow::anyhow!("no RGB32 conversion from {}: {e}", subtype_name(&t.subtype)))?;
    }
    Ok(format!("{}x{}@{:.0} {} -> RGB32", t.w, t.h, t.fps, subtype_name(&t.subtype)))
}

fn read_loop(sh: &CameraShared, reader: &IMFSourceReader) -> Option<(&'static str, String)> {
    let mut seq = 0u64;
    let mut last_ok = std::time::Instant::now();
    loop {
        if sh.stop.load(Ordering::SeqCst) {
            return None;
        }
        let mut flags = 0u32;
        let mut sample: Option<IMFSample> = None;
        let r = unsafe { reader.ReadSample(FIRST_VIDEO_STREAM, 0, None, Some(&mut flags), None, Some(&mut sample)) };
        if let Err(e) = r {
            if sh.stop.load(Ordering::SeqCst) {
                return None;
            }
            let code = if e.code() == MF_E_SHUTDOWN { "CAP.CAMERA.DISCONNECTED" } else { map_hr(e.code()) };
            let code = if code == "CAP.CAMERA.READ_FAILED" { "CAP.CAMERA.DISCONNECTED" } else { code };
            return Some((code, e.message().to_string()));
        }
        if flags & (MF_SOURCE_READERF_ERROR.0 as u32) != 0 {
            return Some(("CAP.CAMERA.DISCONNECTED", "source reader reported a stream error".into()));
        }
        if flags & (MF_SOURCE_READERF_ENDOFSTREAM.0 as u32) != 0 {
            return Some(("CAP.CAMERA.DISCONNECTED", "end of stream".into()));
        }
        let Some(sample) = sample else {
            if last_ok.elapsed() > Duration::from_secs(5) {
                sh.status.set(State::Degraded, Some("CAP.CAMERA.NO_FRAMES"), Some("no frames for 5 s".into()));
            }
            continue;
        };
        match copy_sample(reader, &sample) {
            Ok((w, h, data)) => {
                seq += 1;
                *sh.latest.lock() = Some(Arc::new(CamFrame { width: w, height: h, data, seq }));
                sh.frames.fetch_add(1, Ordering::Relaxed);
                last_ok = std::time::Instant::now();
                if sh.status.state() != State::Active {
                    sh.status.ok(State::Active);
                }
            }
            Err(e) => crate::log!("camera frame copy failed: {e}"),
        }
    }
}

fn copy_sample(reader: &IMFSourceReader, sample: &IMFSample) -> Result<(u32, u32, Vec<u8>)> {
    unsafe {
        let mt = reader.GetCurrentMediaType(FIRST_VIDEO_STREAM)?;
        let size = mt.GetUINT64(&MF_MT_FRAME_SIZE)?;
        let (w, h) = ((size >> 32) as u32, (size & 0xffff_ffff) as u32);
        let buf = sample.ConvertToContiguousBuffer()?;
        let row = w as usize * 4;
        let mut out = vec![0u8; row * h as usize];
        if let Ok(b2) = windows::core::Interface::cast::<IMF2DBuffer>(&buf) {
            let mut p: *mut u8 = std::ptr::null_mut();
            let mut pitch = 0i32;
            b2.Lock2D(&mut p, &mut pitch)?;
            for y in 0..h as usize {
                let src = p.offset(pitch as isize * y as isize);
                std::ptr::copy_nonoverlapping(src, out.as_mut_ptr().add(y * row), row);
            }
            b2.Unlock2D()?;
        } else {
            let mut p: *mut u8 = std::ptr::null_mut();
            let mut len = 0u32;
            buf.Lock(&mut p, None, Some(&mut len))?;
            // Contiguous RGB32 from MF is bottom-up (positive default stride is not guaranteed).
            let stride = mt.GetUINT32(&MF_MT_DEFAULT_STRIDE).map(|s| s as i32).unwrap_or(-(row as i32));
            for y in 0..h as usize {
                let src_row = if stride < 0 { h as usize - 1 - y } else { y };
                if (src_row + 1) * row <= len as usize {
                    std::ptr::copy_nonoverlapping(p.add(src_row * row), out.as_mut_ptr().add(y * row), row);
                }
            }
            buf.Unlock()?;
        }
        Ok((w, h, out))
    }
}
