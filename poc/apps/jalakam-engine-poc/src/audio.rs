//! Microphone capture with WASAPI shared mode, event-driven (ADR-0007).
//!
//! The capture thread runs under MMCSS "Pro Audio" and pushes 48 kHz stereo
//! f32 blocks into a bounded channel owned by the mixer. A full channel drops
//! the block (counted as an overrun); the capture thread never waits.

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

use crossbeam_channel::{Sender, TrySendError};
use jkp_proto::State;
use parking_lot::Mutex;
use windows::core::{w, HRESULT, PCWSTR};
use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
use windows::Win32::Foundation::{CloseHandle, E_ACCESSDENIED, WAIT_OBJECT_0};
use windows::Win32::Media::Audio::*;
use windows::Win32::System::Com::*;
use windows::Win32::System::Threading::{AvRevertMmThreadCharacteristics, AvSetMmThreadCharacteristicsW, CreateEventW, WaitForSingleObject};

use crate::status::StatusCell;

pub const SAMPLE_RATE: u32 = 48_000;
pub const CHANNELS: usize = 2;

pub struct AudioBlock {
    /// Interleaved stereo f32.
    pub samples: Vec<f32>,
}

#[derive(Default)]
pub struct AudioShared {
    pub status: StatusCell,
    pub device: Mutex<String>,
    pub format: Mutex<String>,
    pub blocks: AtomicU64,
    pub overruns: AtomicU64,
    /// f32 bits of the peak absolute sample since the last stats read.
    pub peak: AtomicU32,
    /// Device-clock minus QPC-clock in microseconds (i64 stored as u64 bits).
    pub drift_us: AtomicU64,
    stop: AtomicBool,
}

pub struct AudioCapture {
    pub shared: Arc<AudioShared>,
    thread: Option<JoinHandle<()>>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct EndpointInfo {
    pub flow: &'static str,
    pub name: String,
    pub is_default: bool,
    pub mix_format: String,
}

unsafe fn friendly_name(dev: &IMMDevice) -> String {
    match dev.OpenPropertyStore(STGM_READ) {
        Ok(store) => store.GetValue(&PKEY_Device_FriendlyName).map(|v| v.to_string()).unwrap_or_default(),
        Err(_) => String::new(),
    }
}

unsafe fn mix_format(dev: &IMMDevice) -> String {
    match dev.Activate::<IAudioClient>(CLSCTX_ALL, None) {
        Ok(c) => match c.GetMixFormat() {
            Ok(p) => {
                let f = *p;
                let (sr, ch, bits) = ({ f.nSamplesPerSec }, { f.nChannels }, { f.wBitsPerSample });
                let s = format!("{sr} Hz {ch} ch {bits} bit");
                CoTaskMemFree(Some(p as *const _));
                s
            }
            Err(e) => format!("error {e}"),
        },
        Err(e) => format!("error {e}"),
    }
}

pub fn list_endpoints() -> Vec<EndpointInfo> {
    let mut out = Vec::new();
    unsafe {
        let Ok(en) = CoCreateInstance::<_, IMMDeviceEnumerator>(&MMDeviceEnumerator, None, CLSCTX_ALL) else { return out };
        for (flow, label) in [(eCapture, "capture"), (eRender, "render")] {
            let default_id = en
                .GetDefaultAudioEndpoint(flow, eConsole)
                .and_then(|d| d.GetId())
                .map(|p| {
                    let s = p.to_string().unwrap_or_default();
                    CoTaskMemFree(Some(p.0 as *const _));
                    s
                })
                .unwrap_or_default();
            let Ok(coll) = en.EnumAudioEndpoints(flow, DEVICE_STATE_ACTIVE) else { continue };
            let n = coll.GetCount().unwrap_or(0);
            for i in 0..n {
                let Ok(dev) = coll.Item(i) else { continue };
                let id = dev.GetId().map(|p| {
                    let s = p.to_string().unwrap_or_default();
                    CoTaskMemFree(Some(p.0 as *const _));
                    s
                });
                out.push(EndpointInfo {
                    flow: label,
                    name: friendly_name(&dev),
                    is_default: id.map(|s| s == default_id).unwrap_or(false),
                    mix_format: mix_format(&dev),
                });
            }
        }
    }
    out
}

unsafe fn find_capture(en: &IMMDeviceEnumerator, selector: Option<&str>) -> windows::core::Result<Option<IMMDevice>> {
    let Some(sel) = selector else {
        return en.GetDefaultAudioEndpoint(eCapture, eCommunications).map(Some);
    };
    let sel = sel.to_lowercase();
    let coll = en.EnumAudioEndpoints(eCapture, DEVICE_STATE_ACTIVE)?;
    for i in 0..coll.GetCount()? {
        let dev = coll.Item(i)?;
        if friendly_name(&dev).to_lowercase().contains(&sel) {
            return Ok(Some(dev));
        }
    }
    Ok(None)
}

fn map_hr(hr: HRESULT) -> &'static str {
    if hr == E_ACCESSDENIED {
        "CAP.AUDIO.ACCESS_DENIED"
    } else if hr == AUDCLNT_E_DEVICE_INVALIDATED {
        "CAP.AUDIO.DISCONNECTED"
    } else if hr == AUDCLNT_E_DEVICE_IN_USE {
        "CAP.AUDIO.IN_USE"
    } else if hr == AUDCLNT_E_UNSUPPORTED_FORMAT {
        "CAP.AUDIO.UNSUPPORTED_FORMAT"
    } else {
        "CAP.AUDIO.FAILED"
    }
}

impl AudioCapture {
    pub fn start(selector: Option<String>, tx: Sender<AudioBlock>) -> Self {
        let shared = Arc::new(AudioShared::default());
        shared.status.ok(State::Starting);
        let sh = shared.clone();
        let thread = std::thread::Builder::new()
            .name("mic".into())
            .spawn(move || {
                unsafe {
                    let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
                }
                let mut ever_opened = false;
                while !sh.stop.load(Ordering::SeqCst) {
                    match unsafe { capture_session(&sh, selector.as_deref(), &tx, &mut ever_opened) } {
                        Ok(()) => break,
                        Err((code, detail)) => {
                            let state = if code == "CAP.AUDIO.DISCONNECTED" || ever_opened { State::Degraded } else { State::Failed };
                            sh.status.set(state, Some(code), Some(detail));
                        }
                    }
                    for _ in 0..40 {
                        if sh.stop.load(Ordering::SeqCst) {
                            break;
                        }
                        std::thread::sleep(Duration::from_millis(50));
                    }
                }
                unsafe { CoUninitialize() };
            })
            .expect("spawn mic thread");
        Self { shared, thread: Some(thread) }
    }

    pub fn stop(mut self) {
        self.shared.stop.store(true, Ordering::SeqCst);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
        self.shared.status.ok(State::Idle);
    }
}

/// Returns Ok(()) on a requested stop, Err((code, detail)) on failure.
unsafe fn capture_session(
    sh: &AudioShared,
    selector: Option<&str>,
    tx: &Sender<AudioBlock>,
    ever_opened: &mut bool,
) -> Result<(), (&'static str, String)> {
    let e = |err: windows::core::Error| (map_hr(err.code()), err.message().to_string());
    let en: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).map_err(e)?;
    let dev = match find_capture(&en, selector) {
        Ok(Some(d)) => d,
        Ok(None) => {
            let code = if *ever_opened { "CAP.AUDIO.DISCONNECTED" } else { "CAP.AUDIO.NOT_FOUND" };
            return Err((code, format!("no active capture endpoint matching '{}'", selector.unwrap_or("<default>"))));
        }
        Err(err) => return Err(("CAP.AUDIO.NOT_FOUND", err.message().to_string())),
    };
    *sh.device.lock() = friendly_name(&dev);
    let client: IAudioClient = dev.Activate(CLSCTX_ALL, None).map_err(e)?;
    let wf = WAVEFORMATEX {
        wFormatTag: 3, // WAVE_FORMAT_IEEE_FLOAT
        nChannels: CHANNELS as u16,
        nSamplesPerSec: SAMPLE_RATE,
        nAvgBytesPerSec: SAMPLE_RATE * 8,
        nBlockAlign: 8,
        wBitsPerSample: 32,
        cbSize: 0,
    };
    client
        .Initialize(
            AUDCLNT_SHAREMODE_SHARED,
            AUDCLNT_STREAMFLAGS_EVENTCALLBACK | AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM | AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY,
            1_000_000, // 100 ms buffer, in 100 ns units
            0,
            &wf,
            None,
        )
        .map_err(e)?;
    let event = CreateEventW(None, false, false, PCWSTR::null()).map_err(e)?;
    client.SetEventHandle(event).map_err(e)?;
    let cap: IAudioCaptureClient = client.GetService().map_err(e)?;
    *sh.format.lock() = format!("{SAMPLE_RATE} Hz {CHANNELS} ch f32 (shared, autoconvert)");
    let mut task_index = 0u32;
    let mmcss = AvSetMmThreadCharacteristicsW(w!("Pro Audio"), &mut task_index).ok();
    client.Start().map_err(e)?;
    *ever_opened = true;
    sh.status.ok(State::Active);

    let mut first_qpc: Option<u64> = None;
    let mut frames_total: u64 = 0;
    let result = loop {
        if sh.stop.load(Ordering::SeqCst) {
            break Ok(());
        }
        let w = WaitForSingleObject(event, 200);
        if w != WAIT_OBJECT_0 {
            // No data for 200 ms: check the device still exists.
            if let Err(err) = client.GetCurrentPadding() {
                break Err(e(err));
            }
            continue;
        }
        let mut failed = None;
        loop {
            let n = match cap.GetNextPacketSize() {
                Ok(n) => n,
                Err(err) => {
                    failed = Some(e(err));
                    break;
                }
            };
            if n == 0 {
                break;
            }
            let mut data: *mut u8 = std::ptr::null_mut();
            let mut frames = 0u32;
            let mut flags = 0u32;
            let mut qpc_pos = 0u64;
            if let Err(err) = cap.GetBuffer(&mut data, &mut frames, &mut flags, None, Some(&mut qpc_pos)) {
                failed = Some(e(err));
                break;
            }
            let count = frames as usize * CHANNELS;
            let samples = if flags & (AUDCLNT_BUFFERFLAGS_SILENT.0 as u32) != 0 || data.is_null() {
                vec![0f32; count]
            } else {
                std::slice::from_raw_parts(data as *const f32, count).to_vec()
            };
            let _ = cap.ReleaseBuffer(frames);
            let peak = samples.iter().fold(0f32, |m, s| m.max(s.abs()));
            let prev = f32::from_bits(sh.peak.load(Ordering::Relaxed));
            if peak > prev {
                sh.peak.store(peak.to_bits(), Ordering::Relaxed);
            }
            // qpc_pos is in 100 ns units.
            let first = *first_qpc.get_or_insert(qpc_pos);
            let dev_us = frames_total as i64 * 1_000_000 / SAMPLE_RATE as i64;
            let qpc_us = (qpc_pos.saturating_sub(first) / 10) as i64;
            sh.drift_us.store((dev_us - qpc_us) as u64, Ordering::Relaxed);
            frames_total += frames as u64;
            sh.blocks.fetch_add(1, Ordering::Relaxed);
            match tx.try_send(AudioBlock { samples }) {
                Ok(()) => {}
                Err(TrySendError::Full(_)) => {
                    sh.overruns.fetch_add(1, Ordering::Relaxed);
                }
                Err(TrySendError::Disconnected(_)) => {
                    failed = Some(("CAP.AUDIO.FAILED", "mixer gone".into()));
                    break;
                }
            }
        }
        if let Some(f) = failed {
            break Err(f);
        }
    };
    let _ = client.Stop();
    if let Some(h) = mmcss {
        let _ = AvRevertMmThreadCharacteristics(h);
    }
    let _ = CloseHandle(event);
    result
}
