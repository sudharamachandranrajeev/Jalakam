//! Video encoder sessions (FFmpeg) fed by the render thread (ADR-0004/0014).
//!
//! Hardware path (h264_nvenc, h264_amf): the render thread converts the canvas
//! to NV12 directly into a texture from the encoder's D3D11 frame pool, so the
//! frame never leaves the GPU. Other encoders get NV12/YUV420P system-memory
//! frames read back from a staging ring.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Instant;

use anyhow::{bail, Context, Result};
use crossbeam_channel::{Receiver, Sender};
use jkp_proto::{EncoderStats, State};
use parking_lot::Mutex;
use windows::core::Interface;

use crate::bus::Bus;
use crate::config::VideoOutputConfig;
use crate::ff::{self, sys, CodecPar, Dict, Packet};
use crate::gpu::Gpu;
use crate::status::StatusCell;

/// AVFrame owned by whoever holds this value.
pub struct OwnedFrame(pub *mut sys::AVFrame);
unsafe impl Send for OwnedFrame {}
impl Drop for OwnedFrame {
    fn drop(&mut self) {
        unsafe { sys::av_frame_free(&mut self.0) }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InputKind {
    /// D3D11 NV12 frames from `hw_frames`.
    D3D11,
    Nv12,
    Yuv420p,
}

pub struct VideoEncoderShared {
    pub label: String,
    pub requested: String,
    pub opened: Mutex<String>,
    pub settings: String,
    pub status: StatusCell,
    pub hardware: AtomicBool,
    pub zero_copy: AtomicBool,
    pub frames_in: AtomicU64,
    pub packets_out: AtomicU64,
    pub dropped_queue_full: AtomicU64,
    pub errors: AtomicU64,
    pub encode_ns: AtomicU64,
    pub encode_count: AtomicU64,
    pub force_idr: AtomicBool,
    pub bus: Bus,
}

/// What the render thread needs to feed this encoder.
pub struct VideoSink {
    pub shared: Arc<VideoEncoderShared>,
    pub width: u32,
    pub height: u32,
    pub input: InputKind,
    /// AVBufferRef* of the AVHWFramesContext (D3D11 input only).
    pub hw_frames: *mut sys::AVBufferRef,
    pub tx: Sender<OwnedFrame>,
}
unsafe impl Send for VideoSink {}
unsafe impl Sync for VideoSink {}

impl Drop for VideoSink {
    fn drop(&mut self) {
        if !self.hw_frames.is_null() {
            unsafe { sys::av_buffer_unref(&mut self.hw_frames) }
        }
    }
}

pub struct VideoEncoder {
    pub shared: Arc<VideoEncoderShared>,
    pub sink: Arc<VideoSink>,
    pub codecpar: Arc<CodecPar>,
    thread: Option<JoinHandle<()>>,
}

struct Ctx(*mut sys::AVCodecContext);
unsafe impl Send for Ctx {}
impl Drop for Ctx {
    fn drop(&mut self) {
        unsafe { sys::avcodec_free_context(&mut self.0) }
    }
}

unsafe impl Send for Opened {}

const HW_ZERO_COPY: &[&str] = &["h264_nvenc", "h264_amf"];
const HARDWARE_ONLY: &[&str] = &["h264_nvenc", "h264_amf", "h264_qsv"];

unsafe fn make_hw_frames(gpu: &Gpu, w: u32, h: u32) -> Result<*mut sys::AVBufferRef> {
    let mut dev_ref = sys::av_hwdevice_ctx_alloc(sys::AVHWDeviceType::AV_HWDEVICE_TYPE_D3D11VA);
    anyhow::ensure!(!dev_ref.is_null(), "av_hwdevice_ctx_alloc");
    let dev_ctx = (*dev_ref).data as *mut sys::AVHWDeviceContext;
    let d3d = (*dev_ctx).hwctx as *mut ff::AVD3D11VADeviceContext;
    // FFmpeg takes ownership of one reference.
    let raw = gpu.device.clone().into_raw();
    (*d3d).device = raw;
    if let Err(e) = ff::check(sys::av_hwdevice_ctx_init(dev_ref), "av_hwdevice_ctx_init") {
        sys::av_buffer_unref(&mut dev_ref);
        return Err(e);
    }
    let mut frames_ref = sys::av_hwframe_ctx_alloc(dev_ref);
    sys::av_buffer_unref(&mut dev_ref);
    anyhow::ensure!(!frames_ref.is_null(), "av_hwframe_ctx_alloc");
    let fc = (*frames_ref).data as *mut sys::AVHWFramesContext;
    (*fc).format = sys::AVPixelFormat::AV_PIX_FMT_D3D11;
    (*fc).sw_format = sys::AVPixelFormat::AV_PIX_FMT_NV12;
    (*fc).width = w as i32;
    (*fc).height = h as i32;
    // 0 = separate (non-array) textures. A 16-slice NV12 array with
    // BIND_RENDER_TARGET is rejected by CreateTexture2D (E_INVALIDARG) on the
    // test machine; single textures are accepted. Pool depth stays bounded by
    // the 3-frame input queue plus encoder lookahead.
    (*fc).initial_pool_size = 0;
    let hw = (*fc).hwctx as *mut ff::AVD3D11VAFramesContext;
    (*hw).bind_flags = windows::Win32::Graphics::Direct3D11::D3D11_BIND_RENDER_TARGET.0 as u32;
    if let Err(e) = ff::check(sys::av_hwframe_ctx_init(frames_ref), "av_hwframe_ctx_init") {
        sys::av_buffer_unref(&mut frames_ref);
        return Err(e);
    }
    Ok(frames_ref)
}

unsafe fn supports_pix(codec: *const sys::AVCodec, fmt: sys::AVPixelFormat) -> bool {
    let mut p = (*codec).pix_fmts;
    if p.is_null() {
        return true;
    }
    while *p != sys::AVPixelFormat::AV_PIX_FMT_NONE {
        if *p == fmt {
            return true;
        }
        p = p.add(1);
    }
    false
}

struct Opened {
    ctx: Ctx,
    input: InputKind,
    hw_frames: *mut sys::AVBufferRef,
}

unsafe fn open(name: &str, cfg: &VideoOutputConfig, fps: u32, gpu: &Gpu) -> Result<Opened> {
    let cname = ff::cstr(name);
    let codec = sys::avcodec_find_encoder_by_name(cname.as_ptr());
    if codec.is_null() {
        bail!("ENC.NOT_FOUND: encoder '{name}' is not in this FFmpeg build");
    }
    let ctx = Ctx(sys::avcodec_alloc_context3(codec));
    let c = ctx.0;
    (*c).width = cfg.width as i32;
    (*c).height = cfg.height as i32;
    (*c).time_base = ff::q(1, fps as i32);
    (*c).framerate = ff::q(fps as i32, 1);
    (*c).gop_size = (fps * cfg.keyframe_interval_s.max(1)) as i32;
    (*c).max_b_frames = 0;
    (*c).bit_rate = cfg.bitrate_kbps as i64 * 1000;
    (*c).rc_max_rate = (*c).bit_rate;
    (*c).rc_buffer_size = (*c).bit_rate as i32;
    (*c).flags |= sys::AV_CODEC_FLAG_GLOBAL_HEADER as i32;
    (*c).color_primaries = sys::AVColorPrimaries::AVCOL_PRI_BT709;
    (*c).color_trc = sys::AVColorTransferCharacteristic::AVCOL_TRC_BT709;
    (*c).colorspace = sys::AVColorSpace::AVCOL_SPC_BT709;
    (*c).color_range = sys::AVColorRange::AVCOL_RANGE_MPEG;

    let mut hw_frames = std::ptr::null_mut();
    let input = if HW_ZERO_COPY.contains(&name) {
        hw_frames = make_hw_frames(gpu, cfg.width, cfg.height).context("ENC.HWFRAMES_FAILED")?;
        (*c).pix_fmt = sys::AVPixelFormat::AV_PIX_FMT_D3D11;
        (*c).hw_frames_ctx = sys::av_buffer_ref(hw_frames);
        InputKind::D3D11
    } else if supports_pix(codec, sys::AVPixelFormat::AV_PIX_FMT_NV12) {
        (*c).pix_fmt = sys::AVPixelFormat::AV_PIX_FMT_NV12;
        InputKind::Nv12
    } else {
        (*c).pix_fmt = sys::AVPixelFormat::AV_PIX_FMT_YUV420P;
        InputKind::Yuv420p
    };

    let mut opts = Dict::new();
    match name {
        "h264_nvenc" => {
            opts.set("preset", "p4");
            opts.set("tune", "ll");
            opts.set("rc", "cbr");
            opts.set("forced-idr", "1");
            opts.set("profile", "high");
        }
        "h264_amf" => {
            opts.set("usage", "lowlatency");
            opts.set("rc", "cbr");
        }
        "h264_mf" => {
            opts.set("rate_control", "cbr");
            opts.set("scenario", "live_streaming");
        }
        "libopenh264" => {
            opts.set("rc_mode", "bitrate");
        }
        _ => {}
    }
    let r = sys::avcodec_open2(c, codec, &mut opts.0);
    if r < 0 {
        if !hw_frames.is_null() {
            sys::av_buffer_unref(&mut hw_frames);
        }
        bail!("ENC.OPEN_FAILED: {name}: {} ({r})", ff::err_str(r));
    }
    let left = opts.leftover();
    if !left.is_empty() {
        crate::log!("encoder {name}: options not consumed: {left:?}");
    }
    Ok(Opened { ctx, input, hw_frames })
}

/// Try to open `name` at 1280x720@30 and close it again. Ok(description) if usable.
pub fn probe(name: &str, gpu: &Gpu) -> Result<String> {
    let cfg = VideoOutputConfig {
        target: String::new(),
        format: String::new(),
        encoder: name.into(),
        fallback_encoder: String::new(),
        width: 1280,
        height: 720,
        bitrate_kbps: 2500,
        keyframe_interval_s: 2,
        queue_s: 2,
    };
    let mut o = unsafe { open(name, &cfg, 30, gpu)? };
    if !o.hw_frames.is_null() {
        unsafe { sys::av_buffer_unref(&mut o.hw_frames) }
    }
    Ok(format!(
        "opened; input {:?}; {}",
        o.input,
        if HARDWARE_ONLY.contains(&name) { "hardware encoder" } else { "software or MFT (hardware use not confirmed by FFmpeg)" }
    ))
}

impl VideoEncoder {
    pub fn start(label: &str, cfg: &VideoOutputConfig, fps: u32, gpu: &Gpu) -> Result<Self> {
        let settings = format!(
            "{}x{}@{} {} kbps CBR gop {}s no-B",
            cfg.width, cfg.height, fps, cfg.bitrate_kbps, cfg.keyframe_interval_s
        );
        let shared = Arc::new(VideoEncoderShared {
            label: label.to_string(),
            requested: cfg.encoder.clone(),
            opened: Mutex::new(String::new()),
            settings,
            status: StatusCell::default(),
            hardware: AtomicBool::new(false),
            zero_copy: AtomicBool::new(false),
            frames_in: AtomicU64::new(0),
            packets_out: AtomicU64::new(0),
            dropped_queue_full: AtomicU64::new(0),
            errors: AtomicU64::new(0),
            encode_ns: AtomicU64::new(0),
            encode_count: AtomicU64::new(0),
            force_idr: AtomicBool::new(false),
            bus: Bus::default(),
        });
        shared.status.ok(State::Starting);
        let mut tried = vec![cfg.encoder.clone()];
        if !cfg.fallback_encoder.is_empty() && cfg.fallback_encoder != cfg.encoder {
            tried.push(cfg.fallback_encoder.clone());
        }
        let mut last_err = None;
        let mut opened = None;
        for name in &tried {
            match unsafe { open(name, cfg, fps, gpu) } {
                Ok(o) => {
                    opened = Some((name.clone(), o));
                    break;
                }
                Err(e) => {
                    crate::log!("{label}: encoder {name} failed: {e:#}");
                    shared.errors.fetch_add(1, Ordering::Relaxed);
                    last_err = Some(e);
                }
            }
        }
        let Some((name, o)) = opened else {
            let e = last_err.unwrap();
            shared.status.set(State::Failed, Some("ENC.OPEN_FAILED"), Some(format!("{e:#}")));
            return Err(e);
        };
        if name != cfg.encoder {
            shared.status.set(State::Degraded, Some("ENC.FALLBACK"), Some(format!("{} failed; using {name}", cfg.encoder)));
        } else {
            shared.status.ok(State::Active);
        }
        *shared.opened.lock() = name.clone();
        shared.hardware.store(HARDWARE_ONLY.contains(&name.as_str()), Ordering::Relaxed);
        shared.zero_copy.store(o.input == InputKind::D3D11, Ordering::Relaxed);
        let codecpar = Arc::new(CodecPar::from_ctx(o.ctx.0)?);
        let (tx, rx) = crossbeam_channel::bounded::<OwnedFrame>(3);
        let hw_frames = if o.hw_frames.is_null() { std::ptr::null_mut() } else { unsafe { sys::av_buffer_ref(o.hw_frames) } };
        let sink = Arc::new(VideoSink { shared: shared.clone(), width: cfg.width, height: cfg.height, input: o.input, hw_frames, tx });
        let sh = shared.clone();
        let thread = std::thread::Builder::new()
            .name(format!("enc-{label}"))
            .spawn(move || {
                let mut o = o; // capture the whole Send struct, not its raw-pointer field
                encode_loop(&sh, &o.ctx, rx);
                if !o.hw_frames.is_null() {
                    unsafe { sys::av_buffer_unref(&mut o.hw_frames) }
                }
                drop(o.ctx);
                sh.status.ok(State::Idle);
            })?;
        Ok(Self { shared, sink, codecpar, thread: Some(thread) })
    }

    /// Caller must first remove the sink from the render thread's list. The
    /// channel closes when the last sink reference drops; the thread then
    /// flushes the codec and exits.
    pub fn stop(self) {
        let VideoEncoder { sink, thread, .. } = self;
        drop(sink);
        if let Some(t) = thread {
            let _ = t.join();
        }
    }

    pub fn stats(&self) -> EncoderStats {
        let s = &self.shared;
        let n = s.encode_count.load(Ordering::Relaxed).max(1);
        EncoderStats {
            name: s.label.clone(),
            codec: s.opened.lock().clone(),
            hardware: s.hardware.load(Ordering::Relaxed),
            zero_copy: s.zero_copy.load(Ordering::Relaxed),
            settings: s.settings.clone(),
            status: s.status.get(),
            frames_in: s.frames_in.load(Ordering::Relaxed),
            packets_out: s.packets_out.load(Ordering::Relaxed),
            dropped_queue_full: s.dropped_queue_full.load(Ordering::Relaxed),
            encode_ms_avg: s.encode_ns.load(Ordering::Relaxed) as f64 / n as f64 / 1e6,
            queue_depth: self.sink.tx.len(),
            errors: s.errors.load(Ordering::Relaxed),
        }
    }
}

fn drain(sh: &VideoEncoderShared, ctx: &Ctx, pkt: *mut sys::AVPacket) -> Result<()> {
    loop {
        let r = unsafe { sys::avcodec_receive_packet(ctx.0, pkt) };
        if r == ff::eagain() || r == sys::AVERROR_EOF {
            return Ok(());
        }
        ff::check(r, "avcodec_receive_packet")?;
        let p = Packet::new_ref(pkt, unsafe { (*ctx.0).time_base }, true);
        unsafe { sys::av_packet_unref(pkt) };
        sh.packets_out.fetch_add(1, Ordering::Relaxed);
        if sh.bus.publish(&p) {
            sh.force_idr.store(true, Ordering::Relaxed);
        }
    }
}

fn encode_loop(sh: &VideoEncoderShared, ctx: &Ctx, rx: Receiver<OwnedFrame>) {
    let mut pkt = unsafe { sys::av_packet_alloc() };
    let mut consecutive_errors = 0;
    while let Ok(frame) = rx.recv() {
        sh.frames_in.fetch_add(1, Ordering::Relaxed);
        if sh.force_idr.swap(false, Ordering::Relaxed) {
            unsafe { (*frame.0).pict_type = sys::AVPictureType::AV_PICTURE_TYPE_I };
        }
        let t = Instant::now();
        let r = unsafe { sys::avcodec_send_frame(ctx.0, frame.0) };
        drop(frame);
        let res = if r < 0 { Err(anyhow::anyhow!("avcodec_send_frame: {}", ff::err_str(r))) } else { drain(sh, ctx, pkt) };
        sh.encode_ns.fetch_add(t.elapsed().as_nanos() as u64, Ordering::Relaxed);
        sh.encode_count.fetch_add(1, Ordering::Relaxed);
        match res {
            Ok(()) => {
                consecutive_errors = 0;
                if sh.status.state() == State::Failed {
                    sh.status.ok(State::Active);
                }
            }
            Err(e) => {
                sh.errors.fetch_add(1, Ordering::Relaxed);
                consecutive_errors += 1;
                let state = if consecutive_errors > 30 { State::Failed } else { State::Degraded };
                sh.status.set(state, Some("ENC.ENCODE_FAILED"), Some(format!("{e:#}")));
            }
        }
    }
    // Flush delayed packets so the outputs receive the tail.
    unsafe {
        sys::avcodec_send_frame(ctx.0, std::ptr::null());
    }
    let _ = drain(sh, ctx, pkt);
    unsafe { sys::av_packet_free(&mut pkt) };
}
