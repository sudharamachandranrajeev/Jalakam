//! Outputs: RTMP (FLV) and local recording (Matroska), one thread each
//! (ADR-0014). Each output owns a bounded packet queue, rebases timestamps
//! to its own start, waits for a keyframe, and for network targets runs the
//! reconnect state machine with exponential backoff.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use anyhow::Result;
use crossbeam_channel::{Receiver, RecvTimeoutError};
use jkp_proto::{OutputStats, State};
use parking_lot::Mutex;

use crate::bus::{Bus, Subscriber};
use crate::ff::{self, sys, CodecPar, Dict, Packet};
use crate::status::StatusCell;

const US: sys::AVRational = ff::q(1, 1_000_000);
const MAX_ATTEMPTS: u32 = 25;
const BACKOFF_BASE_S: u64 = 2;
/// POC cap (ADR-0014 specifies 15 min for the product; shortened for tests).
const BACKOFF_CAP_S: u64 = 30;

pub struct OutputShared {
    pub name: String,
    pub target: String,
    pub status: StatusCell,
    pub bytes: AtomicU64,
    pub video_packets: AtomicU64,
    pub audio_packets: AtomicU64,
    pub dropped: AtomicU64,
    pub reconnects: AtomicU64,
    pub started: Mutex<Option<Instant>>,
    pub sub: Arc<Subscriber>,
    stop: AtomicBool,
    last_bytes: Mutex<(Instant, u64)>,
}

pub struct Output {
    pub shared: Arc<OutputShared>,
    thread: Option<JoinHandle<()>>,
}

pub struct OutputSpec {
    pub name: String,
    pub target: String,
    pub format: String,
    pub network: bool,
    pub video: Arc<CodecPar>,
    pub audio: Arc<CodecPar>,
    pub queue_packets: usize,
}

struct Mux {
    oc: *mut sys::AVFormatContext,
    vst: i32,
    ast: i32,
    header_written: bool,
}

impl Mux {
    unsafe fn open(spec: &OutputSpec) -> Result<Mux> {
        let fmt = ff::cstr(&spec.format);
        let url = ff::cstr(&spec.target);
        let mut oc = std::ptr::null_mut();
        ff::check(
            sys::avformat_alloc_output_context2(&mut oc, std::ptr::null(), fmt.as_ptr(), url.as_ptr()),
            "avformat_alloc_output_context2",
        )?;
        let mut m = Mux { oc, vst: -1, ast: -1, header_written: false };
        for (par, is_video) in [(&spec.video, true), (&spec.audio, false)] {
            let st = sys::avformat_new_stream(oc, std::ptr::null());
            anyhow::ensure!(!st.is_null(), "avformat_new_stream");
            ff::check(sys::avcodec_parameters_copy((*st).codecpar, par.par), "avcodec_parameters_copy")?;
            (*(*st).codecpar).codec_tag = 0;
            (*st).time_base = par.time_base;
            if is_video {
                m.vst = (*st).index;
            } else {
                m.ast = (*st).index;
            }
        }
        if (*(*oc).oformat).flags & sys::AVFMT_NOFILE as i32 == 0 {
            let mut opts = Dict::new();
            if spec.network {
                opts.set("rw_timeout", "5000000"); // 5 s: detect a stalled peer
                opts.set("tcp_nodelay", "1");
            }
            ff::check(
                sys::avio_open2(&mut (*oc).pb, url.as_ptr(), sys::AVIO_FLAG_WRITE as i32, std::ptr::null(), &mut opts.0),
                "avio_open2",
            )?;
        }
        let mut hopts = Dict::new();
        if spec.format == "flv" {
            hopts.set("flvflags", "no_duration_filesize");
        }
        ff::check(sys::avformat_write_header(oc, &mut hopts.0), "avformat_write_header")?;
        m.header_written = true;
        Ok(m)
    }

    unsafe fn write(&mut self, p: &Packet, pts_us: i64) -> Result<usize> {
        let st_index = if p.is_video { self.vst } else { self.ast };
        let st = *(*self.oc).streams.add(st_index as usize);
        let out = sys::av_packet_clone(p.pkt);
        let size = (*out).size as usize;
        (*out).stream_index = st_index;
        let ts = ff::rescale(pts_us, US, (*st).time_base);
        (*out).pts = ts;
        (*out).dts = ts;
        (*out).duration = sys::av_rescale_q((*out).duration, p.time_base, (*st).time_base);
        let r = sys::av_interleaved_write_frame(self.oc, out);
        let mut out = out;
        sys::av_packet_free(&mut out);
        ff::check(r, "av_interleaved_write_frame")?;
        Ok(size)
    }

    unsafe fn close(mut self) -> Result<()> {
        let mut res = Ok(());
        if self.header_written {
            let r = sys::av_write_trailer(self.oc);
            if r < 0 {
                res = Err(anyhow::anyhow!("av_write_trailer: {}", ff::err_str(r)));
            }
        }
        if !(*self.oc).pb.is_null() && (*(*self.oc).oformat).flags & sys::AVFMT_NOFILE as i32 == 0 {
            sys::avio_closep(&mut (*self.oc).pb);
        }
        sys::avformat_free_context(self.oc);
        self.oc = std::ptr::null_mut();
        res
    }
}

impl Output {
    pub fn start(spec: OutputSpec, video_bus: &Bus, audio_bus: &Bus) -> Self {
        let (sub, rx) = Subscriber::new(spec.queue_packets);
        let shared = Arc::new(OutputShared {
            name: spec.name.clone(),
            target: redact(&spec.target),
            status: StatusCell::default(),
            bytes: AtomicU64::new(0),
            video_packets: AtomicU64::new(0),
            audio_packets: AtomicU64::new(0),
            dropped: AtomicU64::new(0),
            reconnects: AtomicU64::new(0),
            started: Mutex::new(None),
            sub: sub.clone(),
            stop: AtomicBool::new(false),
            last_bytes: Mutex::new((Instant::now(), 0)),
        });
        shared.status.ok(State::Starting);
        video_bus.subscribe(sub.clone());
        audio_bus.subscribe(sub);
        let sh = shared.clone();
        let thread = std::thread::Builder::new()
            .name(format!("out-{}", spec.name))
            .spawn(move || {
                run(&sh, &spec, rx);
            })
            .expect("spawn output thread");
        Self { shared, thread: Some(thread) }
    }

    /// Unsubscribe, write the queued tail, close the muxer. Returns final stats.
    pub fn stop(mut self, video_bus: &Bus, audio_bus: &Bus) -> OutputStats {
        video_bus.unsubscribe(&self.shared.sub);
        audio_bus.unsubscribe(&self.shared.sub);
        self.shared.stop.store(true, Ordering::SeqCst);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
        self.stats()
    }

    pub fn stats(&self) -> OutputStats {
        let s = &self.shared;
        let bytes = s.bytes.load(Ordering::Relaxed);
        let kbps = {
            let mut lb = s.last_bytes.lock();
            let dt = lb.0.elapsed().as_secs_f64();
            let k = if dt > 0.0 { (bytes.saturating_sub(lb.1)) as f64 * 8.0 / dt / 1000.0 } else { 0.0 };
            *lb = (Instant::now(), bytes);
            k
        };
        OutputStats {
            name: s.name.clone(),
            target: s.target.clone(),
            status: s.status.get(),
            bytes,
            kbps,
            video_packets: s.video_packets.load(Ordering::Relaxed),
            audio_packets: s.audio_packets.load(Ordering::Relaxed),
            dropped_packets: s.dropped.load(Ordering::Relaxed) + s.sub.dropped.load(Ordering::Relaxed),
            reconnects: s.reconnects.load(Ordering::Relaxed),
            uptime_s: s.started.lock().map(|t| t.elapsed().as_secs_f64()).unwrap_or(0.0),
            queue_depth: s.sub.depth(),
        }
    }
}

/// Hide the stream key (last path segment) in logs and stats (threat model T-25).
pub fn redact(url: &str) -> String {
    if url.starts_with("rtmp") {
        if let Some(i) = url.rfind('/') {
            return format!("{}/***", &url[..i]);
        }
    }
    url.to_string()
}

fn sleep_draining(sh: &OutputShared, rx: &Receiver<Packet>, d: Duration) -> bool {
    let end = Instant::now() + d;
    while Instant::now() < end {
        if sh.stop.load(Ordering::SeqCst) {
            return false;
        }
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(_) => {
                sh.dropped.fetch_add(1, Ordering::Relaxed);
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return false,
        }
    }
    true
}

fn run(sh: &OutputShared, spec: &OutputSpec, rx: Receiver<Packet>) {
    let kind = if spec.network { "OUT.RTMP" } else { "REC" };
    let mut attempt: u32 = 0;
    'connect: loop {
        if sh.stop.load(Ordering::SeqCst) {
            break;
        }
        let mux = match unsafe { Mux::open(spec) } {
            Ok(m) => m,
            Err(e) => {
                if !spec.network {
                    sh.status.set(State::Failed, Some("REC.OPEN_FAILED"), Some(format!("{e:#}")));
                    return;
                }
                attempt += 1;
                if attempt > MAX_ATTEMPTS {
                    sh.status.set(State::Failed, Some("OUT.RTMP.GAVE_UP"), Some(format!("{MAX_ATTEMPTS} attempts: {e:#}")));
                    return;
                }
                let wait = (BACKOFF_BASE_S << (attempt - 1).min(5)).min(BACKOFF_CAP_S);
                let code = if format!("{e:#}").contains("Permission denied") || format!("{e:#}").contains("Forbidden") {
                    "OUT.RTMP.AUTH_REJECTED"
                } else {
                    "OUT.RTMP.CONNECT_FAILED"
                };
                sh.status.set(State::Reconnecting, Some(code), Some(format!("attempt {attempt}, retry in {wait}s: {e:#}")));
                if !sleep_draining(sh, &rx, Duration::from_secs(wait)) {
                    break;
                }
                continue;
            }
        };
        let mut mux = mux;
        let reconnecting = attempt > 0;
        attempt = 0;
        sh.sub.needs_key.store(true, Ordering::Relaxed);
        // Connected is not yet live: the output becomes Active when the first
        // keyframe is on the wire (the bus forces an IDR for this subscriber).
        let connected_at = Instant::now();
        if reconnecting {
            sh.status.set(State::Reconnecting, None, Some("connected; waiting for keyframe".into()));
        } else {
            sh.status.set(State::Starting, None, Some("connected; waiting for keyframe".into()));
        }
        let mut live = false;
        let mut base_us: Option<i64> = None;
        loop {
            // On stop, write what is already queued (the encoder flush tail), then close.
            let p = if sh.stop.load(Ordering::SeqCst) {
                match rx.try_recv() {
                    Ok(p) => p,
                    Err(_) => break,
                }
            } else {
                match rx.recv_timeout(Duration::from_millis(200)) {
                    Ok(p) => p,
                    Err(RecvTimeoutError::Timeout) => continue,
                    Err(RecvTimeoutError::Disconnected) => break,
                }
            };
            let pts_us = ff::rescale(p.pts(), p.time_base, US);
            let base = match base_us {
                Some(b) => b,
                None if p.is_video && p.is_key() => {
                    base_us = Some(pts_us);
                    pts_us
                }
                None => {
                    sh.dropped.fetch_add(1, Ordering::Relaxed);
                    continue;
                }
            };
            if pts_us < base {
                sh.dropped.fetch_add(1, Ordering::Relaxed);
                continue;
            }
            match unsafe { mux.write(&p, pts_us - base) } {
                Ok(n) => {
                    if !live {
                        live = true;
                        if reconnecting {
                            sh.reconnects.fetch_add(1, Ordering::Relaxed);
                        }
                        sh.started.lock().get_or_insert_with(Instant::now);
                        sh.status.ok(State::Active);
                        crate::log!("{kind} {}: live {} ms after connect", sh.name, connected_at.elapsed().as_millis());
                    }
                    sh.bytes.fetch_add(n as u64, Ordering::Relaxed);
                    if p.is_video {
                        sh.video_packets.fetch_add(1, Ordering::Relaxed);
                    } else {
                        sh.audio_packets.fetch_add(1, Ordering::Relaxed);
                    }
                }
                Err(e) => {
                    let _ = unsafe { mux.close() };
                    if spec.network {
                        sh.status.set(State::Reconnecting, Some("OUT.RTMP.WRITE_FAILED"), Some(format!("{e:#}")));
                        attempt = 1;
                        if !sleep_draining(sh, &rx, Duration::from_secs(BACKOFF_BASE_S)) {
                            break 'connect;
                        }
                        continue 'connect;
                    }
                    sh.status.set(State::Failed, Some("REC.WRITE_FAILED"), Some(format!("{e:#}")));
                    return;
                }
            }
        }
        if let Err(e) = unsafe { mux.close() } {
            crate::log!("{kind} {}: close: {e:#}", sh.name);
        }
        break;
    }
    sh.status.ok(State::Idle);
}
