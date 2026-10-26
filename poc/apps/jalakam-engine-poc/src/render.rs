//! The render thread: the only thread that draws on the D3D11 context
//! (ADR-0006). Per tick it polls display capture, uploads the newest camera
//! frame, draws the overlay, composites the canvas, publishes a preview frame
//! and converts the canvas to NV12 for every active encoder. Nothing on this
//! thread waits for the UI, encoders or outputs.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Instant;

use anyhow::Result;
use jkp_proto::State;
use parking_lot::Mutex;
use windows::core::Interface;
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::System::SystemInformation::GetLocalTime;
use windows::Win32::System::Threading::*;

use crate::camera::CameraShared;
use crate::clock::Clock;
use crate::config::Config;
use crate::display::DisplayCapture;
use crate::encoder::{InputKind, OwnedFrame, VideoSink};
use crate::ff::sys;
use crate::gpu::{self, Gpu};
use crate::overlay::Overlay;
use crate::preview::PreviewRing;
use crate::quad::{QuadPipeline, Rect};
use crate::status::StatusCell;

#[derive(Default)]
pub struct RenderShared {
    pub ticks: AtomicU64,
    pub lagged: AtomicU64,
    pub tick_ns_total: AtomicU64,
    pub tick_ns_max: AtomicU64,
    pub gpu_ns_total: AtomicU64,
    pub gpu_samples: AtomicU64,
    pub preview_skipped: AtomicU64,
    /// Rolling per-tick samples (µs) for the 10 s percentiles.
    pub tick_us: Mutex<Ring>,
    pub gpu_us: Mutex<Ring>,
    pub composite_us: Mutex<Ring>,
    pub convert_us: Mutex<Ring>,
    pub jitter_us: Mutex<Ring>,
    pub display_status: StatusCell,
    pub display_frames: AtomicU64,
    pub display_format: Mutex<String>,
    pub sinks: Mutex<Vec<Arc<VideoSink>>>,
    pub camera: Mutex<Option<Arc<CameraShared>>>,
    stop: AtomicBool,
}

/// Fixed-size ring of the most recent samples.
pub struct Ring {
    buf: Vec<u32>,
    next: usize,
}

impl Default for Ring {
    fn default() -> Self {
        Self { buf: Vec::with_capacity(RING_LEN), next: 0 }
    }
}

const RING_LEN: usize = 600;

impl Ring {
    pub fn push(&mut self, v: u32) {
        if self.buf.len() < RING_LEN {
            self.buf.push(v);
        } else {
            self.buf[self.next] = v;
        }
        self.next = (self.next + 1) % RING_LEN;
    }
    pub fn mean_ms(&self) -> f64 {
        if self.buf.is_empty() {
            return 0.0;
        }
        self.buf.iter().map(|v| *v as f64).sum::<f64>() / self.buf.len() as f64 / 1000.0
    }
    /// Percentile in ms (nearest rank).
    pub fn pct_ms(&self, p: f64) -> f64 {
        if self.buf.is_empty() {
            return 0.0;
        }
        let mut v = self.buf.clone();
        v.sort_unstable();
        let i = ((p / 100.0) * v.len() as f64).ceil() as usize;
        v[i.clamp(1, v.len()) - 1] as f64 / 1000.0
    }
}

pub struct Renderer {
    pub shared: Arc<RenderShared>,
    thread: Option<JoinHandle<()>>,
}

impl Renderer {
    pub fn start(cfg: Config, gpu: Arc<Gpu>, clock: Clock, shared: Arc<RenderShared>) -> Result<Self> {
        let sh = shared.clone();
        let (ready_tx, ready_rx) = crossbeam_channel::bounded::<Result<()>>(1);
        let thread = std::thread::Builder::new().name("render".into()).spawn(move || {
            unsafe {
                let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_MULTITHREADED);
                let _ = SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_HIGHEST);
            }
            match RenderState::new(&cfg, &gpu, &sh) {
                Ok(mut st) => {
                    let _ = ready_tx.send(Ok(()));
                    st.run(&cfg, &gpu, clock, &sh);
                }
                Err(e) => {
                    let _ = ready_tx.send(Err(e));
                }
            }
            unsafe { windows::Win32::System::Com::CoUninitialize() };
        })?;
        ready_rx.recv()??;
        Ok(Self { shared, thread: Some(thread) })
    }

    pub fn stop(mut self) {
        self.shared.stop.store(true, Ordering::SeqCst);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

struct CameraTex {
    tex: ID3D11Texture2D,
    srv: ID3D11ShaderResourceView,
    w: u32,
    h: u32,
    seq: u64,
}

struct SwPath {
    rt: ID3D11Texture2D,
    out_view: ID3D11VideoProcessorOutputView,
    staging: Vec<ID3D11Texture2D>,
    pending: Vec<Option<i64>>,
    idx: usize,
}

struct SinkGpu {
    sink: Arc<VideoSink>,
    vp: ID3D11VideoProcessor,
    en: ID3D11VideoProcessorEnumerator,
    in_view: ID3D11VideoProcessorInputView,
    hw_views: HashMap<(usize, u32), ID3D11VideoProcessorOutputView>,
    sw: Option<SwPath>,
}

struct GpuTimer {
    disjoint: Vec<ID3D11Query>,
    begin: Vec<ID3D11Query>,
    compose: Vec<ID3D11Query>,
    mid: Vec<ID3D11Query>,
    end: Vec<ID3D11Query>,
    frame: usize,
}

struct RenderState {
    quad: QuadPipeline,
    canvas: ID3D11Texture2D,
    canvas_rtv: ID3D11RenderTargetView,
    canvas_srv: ID3D11ShaderResourceView,
    display: Option<DisplayCapture>,
    display_retry_at: u64,
    camera_tex: Option<CameraTex>,
    overlay: Overlay,
    preview: PreviewRing,
    sinks: HashMap<usize, SinkGpu>,
    vdev: ID3D11VideoDevice,
    vctx: ID3D11VideoContext1,
    timer: GpuTimer,
}

fn query(dev: &ID3D11Device, kind: D3D11_QUERY) -> Result<ID3D11Query> {
    let mut q = None;
    unsafe { dev.CreateQuery(&D3D11_QUERY_DESC { Query: kind, MiscFlags: 0 }, Some(&mut q))? };
    Ok(q.unwrap())
}

impl RenderState {
    fn new(cfg: &Config, gpu: &Gpu, sh: &RenderShared) -> Result<Self> {
        let dev = &gpu.device;
        let (cw, ch) = (cfg.canvas.width, cfg.canvas.height);
        let canvas = gpu::create_texture(
            dev,
            &gpu::texture_desc(
                cw,
                ch,
                DXGI_FORMAT_B8G8R8A8_UNORM,
                D3D11_BIND_FLAG(D3D11_BIND_RENDER_TARGET.0 | D3D11_BIND_SHADER_RESOURCE.0),
                D3D11_RESOURCE_MISC_FLAG(0),
            ),
        )?;
        let pw = cfg.canvas.preview_width & !1;
        let ph = ((pw as u64 * ch as u64 / cw as u64) as u32) & !1;
        let mut t = GpuTimer { disjoint: vec![], begin: vec![], compose: vec![], mid: vec![], end: vec![], frame: 0 };
        for _ in 0..4 {
            t.disjoint.push(query(dev, D3D11_QUERY_TIMESTAMP_DISJOINT)?);
            t.begin.push(query(dev, D3D11_QUERY_TIMESTAMP)?);
            t.compose.push(query(dev, D3D11_QUERY_TIMESTAMP)?);
            t.mid.push(query(dev, D3D11_QUERY_TIMESTAMP)?);
            t.end.push(query(dev, D3D11_QUERY_TIMESTAMP)?);
        }
        let mut st = Self {
            quad: QuadPipeline::new(dev)?,
            canvas_rtv: gpu::create_rtv(dev, &canvas)?,
            canvas_srv: gpu::create_srv(dev, &canvas)?,
            canvas,
            display: None,
            display_retry_at: 0,
            camera_tex: None,
            overlay: Overlay::new(dev, 1100, 56)?,
            preview: PreviewRing::new(dev, pw, ph, gpu.adapter_luid)?,
            sinks: HashMap::new(),
            vdev: dev.cast()?,
            vctx: gpu.context.cast()?,
            timer: t,
        };
        st.try_display(cfg, gpu, sh);
        Ok(st)
    }

    fn try_display(&mut self, cfg: &Config, gpu: &Gpu, sh: &RenderShared) {
        let Some(idx) = cfg.sources.display else {
            sh.display_status.ok(State::Idle);
            return;
        };
        match DisplayCapture::start(&gpu.device, idx) {
            Ok(d) => {
                *sh.display_format.lock() = format!("{} {}x{} BGRA (WGC)", d.name, d.width, d.height);
                sh.display_status.ok(State::Active);
                self.display = Some(d);
            }
            Err(e) => {
                let msg = format!("{e:#}");
                let code = msg.split(':').next().filter(|c| c.starts_with("CAP.")).unwrap_or("CAP.DISPLAY.FAILED").to_string();
                sh.display_status.set(State::Failed, Some(&code), Some(msg));
            }
        }
    }

    fn run(&mut self, cfg: &Config, gpu: &Gpu, clock: Clock, sh: &RenderShared) {
        let fps = cfg.canvas.fps as i64;
        let period_us = 1_000_000 / fps;
        let timer = unsafe {
            CreateWaitableTimerExW(None, None, CREATE_WAITABLE_TIMER_HIGH_RESOLUTION, TIMER_ALL_ACCESS.0).unwrap_or(HANDLE::default())
        };
        let mut n: i64 = clock.now_us() / period_us + 1;
        while !sh.stop.load(Ordering::SeqCst) {
            let due = n * period_us;
            // Sleep to SPIN_US before the deadline, then spin. The high-resolution
            // timer alone overshot by > 1 ms p99 in 26 % of 10 s windows (30 min run).
            const SPIN_US: i64 = 300;
            let wait = due - SPIN_US - clock.now_us();
            if wait > 0 {
                unsafe {
                    let rel = -(wait * 10); // 100 ns units, negative = relative
                    if !timer.is_invalid() && SetWaitableTimerEx(timer, &rel, 0, None, None, None, 0).is_ok() {
                        WaitForSingleObject(timer, 1000);
                    } else {
                        std::thread::sleep(std::time::Duration::from_micros(wait as u64));
                    }
                }
            }
            while clock.now_us() < due && !sh.stop.load(Ordering::Relaxed) {
                std::thread::yield_now();
            }
            let late = clock.now_us() - due;
            sh.jitter_us.lock().push(late.clamp(0, u32::MAX as i64) as u32);
            if late > period_us {
                let missed = late / period_us;
                sh.lagged.fetch_add(missed as u64, Ordering::Relaxed);
                n += missed;
            }
            let t = Instant::now();
            if let Err(e) = self.tick(cfg, gpu, sh, n) {
                crate::log!("render tick error: {e:#}");
            }
            let ns = t.elapsed().as_nanos() as u64;
            sh.tick_ns_total.fetch_add(ns, Ordering::Relaxed);
            sh.tick_ns_max.fetch_max(ns, Ordering::Relaxed);
            sh.tick_us.lock().push((ns / 1000) as u32);
            sh.ticks.fetch_add(1, Ordering::Relaxed);
            n += 1;
        }
        self.sinks.clear();
        unsafe {
            if !timer.is_invalid() {
                let _ = CloseHandle(timer);
            }
            gpu.context.ClearState();
            gpu.context.Flush();
        }
    }

    fn tick(&mut self, cfg: &Config, gpu: &Gpu, sh: &RenderShared, n: i64) -> Result<()> {
        let ctx = &gpu.context;
        let dev = &gpu.device;
        let (cw, ch) = (cfg.canvas.width, cfg.canvas.height);
        let ti = self.timer.frame % 4;
        unsafe {
            ctx.Begin(&self.timer.disjoint[ti]);
            ctx.End(&self.timer.begin[ti]);
        }

        // Sources
        if self.display.is_none() && cfg.sources.display.is_some() && n as u64 >= self.display_retry_at {
            self.try_display(cfg, gpu, sh);
            self.display_retry_at = n as u64 + 5 * cfg.canvas.fps as u64;
        }
        if let Some(d) = self.display.as_mut() {
            match d.poll(ctx) {
                Ok(true) => {
                    sh.display_frames.store(d.frames, Ordering::Relaxed);
                }
                Ok(false) => {}
                Err(e) => {
                    sh.display_status.set(State::Degraded, Some("CAP.DISPLAY.LOST"), Some(format!("{e:#}")));
                    self.display = None;
                }
            }
        }
        let cam = sh.camera.lock().clone();
        let cam_state = cam.as_ref().map(|c| c.status.get());
        let cam_frame = cam.as_ref().and_then(|c| c.latest.lock().clone());
        match &cam_frame {
            Some(f) => self.upload_camera(dev, ctx, f)?,
            None => self.camera_tex = None,
        }

        // Overlay text
        let st = unsafe { GetLocalTime() };
        let cam_txt = match &cam_state {
            None => "off".to_string(),
            Some(s) => s.code.clone().unwrap_or_else(|| format!("{:?}", s.state).to_lowercase()),
        };
        self.overlay.draw(&format!(
            "{}  {:02}:{:02}:{:02}.{:03}  #{}  cam:{}",
            cfg.overlay.text, st.wHour, st.wMinute, st.wSecond, st.wMilliseconds, n, cam_txt
        ))?;

        // Composite
        unsafe { ctx.End(&self.timer.compose[ti]) };
        let full = Rect { x: 0.0, y: 0.0, w: cw as f32, h: ch as f32 };
        self.quad.begin(ctx, &self.canvas_rtv, cw, ch, [0.06, 0.06, 0.08, 1.0]);
        if let Some(d) = &self.display {
            self.quad.draw(ctx, &d.srv, Rect::fit(d.width as f32, d.height as f32, full), cw, ch, 1.0, true)?;
        }
        if let Some(c) = &self.camera_tex {
            let pip_w = cw as f32 * 0.28;
            let pip_h = pip_w * c.h as f32 / c.w as f32;
            let m = cw as f32 * 0.02;
            let r = Rect { x: cw as f32 - pip_w - m, y: ch as f32 - pip_h - m, w: pip_w, h: pip_h };
            self.quad.draw(ctx, &c.srv, r, cw, ch, 1.0, true)?;
        }
        let m = cw as f32 * 0.02;
        let ov = Rect { x: m, y: m, w: self.overlay.width as f32, h: self.overlay.height as f32 };
        self.quad.draw(ctx, &self.overlay.srv, ov, cw, ch, 1.0, false)?;
        unsafe { ctx.OMSetRenderTargets(None, None) };

        // Preview (dropped first under load; never waits)
        self.preview.publish(ctx, &self.quad, &self.canvas_srv, cw, ch)?;
        sh.preview_skipped.store(self.preview.skipped, Ordering::Relaxed);
        unsafe { ctx.End(&self.timer.mid[ti]) };

        // Encoders
        self.sync_sinks(sh, dev, cfg)?;
        let keys: Vec<usize> = self.sinks.keys().copied().collect();
        for k in keys {
            if let Err(e) = self.feed(k, ctx, n) {
                let s = &self.sinks[&k].sink.shared;
                s.errors.fetch_add(1, Ordering::Relaxed);
                s.status.set(State::Degraded, Some("ENC.INPUT_FAILED"), Some(format!("{e:#}")));
            }
        }

        unsafe {
            ctx.End(&self.timer.end[ti]);
            ctx.End(&self.timer.disjoint[ti]);
            ctx.Flush();
        }
        self.read_timer(ctx, sh);
        self.timer.frame += 1;
        Ok(())
    }

    fn read_timer(&mut self, ctx: &ID3D11DeviceContext, sh: &RenderShared) {
        if self.timer.frame < 3 {
            return;
        }
        let i = (self.timer.frame - 3) % 4;
        unsafe {
            let mut dj = D3D11_QUERY_DATA_TIMESTAMP_DISJOINT::default();
            let (mut b, mut c, mut m, mut e) = (0u64, 0u64, 0u64, 0u64);
            let flag = D3D11_ASYNC_GETDATA_DONOTFLUSH.0 as u32;
            if ctx.GetData(&self.timer.disjoint[i], Some(&mut dj as *mut _ as *mut _), std::mem::size_of_val(&dj) as u32, flag).is_ok()
                && ctx.GetData(&self.timer.begin[i], Some(&mut b as *mut _ as *mut _), 8, flag).is_ok()
                && ctx.GetData(&self.timer.compose[i], Some(&mut c as *mut _ as *mut _), 8, flag).is_ok()
                && ctx.GetData(&self.timer.mid[i], Some(&mut m as *mut _ as *mut _), 8, flag).is_ok()
                && ctx.GetData(&self.timer.end[i], Some(&mut e as *mut _ as *mut _), 8, flag).is_ok()
                && !dj.Disjoint.as_bool()
                && dj.Frequency > 0
                && e > b
            {
                let ns = (e - b) as u128 * 1_000_000_000 / dj.Frequency as u128;
                sh.gpu_ns_total.fetch_add(ns as u64, Ordering::Relaxed);
                sh.gpu_samples.fetch_add(1, Ordering::Relaxed);
                sh.gpu_us.lock().push((ns / 1000) as u32);
                let us = |a: u64, z: u64| (z.saturating_sub(a) as u128 * 1_000_000 / dj.Frequency as u128) as u32;
                sh.composite_us.lock().push(us(c, m));
                sh.convert_us.lock().push(us(m, e));
            }
        }
    }

    fn upload_camera(&mut self, dev: &ID3D11Device, ctx: &ID3D11DeviceContext, f: &crate::camera::CamFrame) -> Result<()> {
        if let Some(c) = &self.camera_tex {
            if c.seq == f.seq && c.w == f.width && c.h == f.height {
                return Ok(());
            }
        }
        if self.camera_tex.as_ref().map(|c| c.w != f.width || c.h != f.height).unwrap_or(true) {
            let mut d = gpu::texture_desc(f.width, f.height, DXGI_FORMAT_B8G8R8X8_UNORM, D3D11_BIND_SHADER_RESOURCE, D3D11_RESOURCE_MISC_FLAG(0));
            d.Usage = D3D11_USAGE_DYNAMIC;
            d.CPUAccessFlags = D3D11_CPU_ACCESS_WRITE.0 as u32;
            let tex = gpu::create_texture(dev, &d)?;
            let srv = gpu::create_srv(dev, &tex)?;
            self.camera_tex = Some(CameraTex { tex, srv, w: f.width, h: f.height, seq: 0 });
        }
        let c = self.camera_tex.as_mut().unwrap();
        unsafe {
            let mut m = D3D11_MAPPED_SUBRESOURCE::default();
            ctx.Map(&c.tex, 0, D3D11_MAP_WRITE_DISCARD, 0, Some(&mut m))?;
            let row = f.width as usize * 4;
            for y in 0..f.height as usize {
                std::ptr::copy_nonoverlapping(f.data.as_ptr().add(y * row), (m.pData as *mut u8).add(y * m.RowPitch as usize), row);
            }
            ctx.Unmap(&c.tex, 0);
        }
        c.seq = f.seq;
        Ok(())
    }

    fn sync_sinks(&mut self, sh: &RenderShared, dev: &ID3D11Device, cfg: &Config) -> Result<()> {
        let current: Vec<Arc<VideoSink>> = sh.sinks.lock().clone();
        let live: Vec<usize> = current.iter().map(|s| Arc::as_ptr(s) as usize).collect();
        self.sinks.retain(|k, _| live.contains(k));
        for s in current {
            let k = Arc::as_ptr(&s) as usize;
            if self.sinks.contains_key(&k) {
                continue;
            }
            match self.make_sink(dev, s.clone(), cfg) {
                Ok(g) => {
                    self.sinks.insert(k, g);
                }
                Err(e) => {
                    s.shared.status.set(State::Failed, Some("ENC.INPUT_SETUP_FAILED"), Some(format!("{e:#}")));
                    s.shared.errors.fetch_add(1, Ordering::Relaxed);
                }
            }
        }
        Ok(())
    }

    fn make_sink(&self, dev: &ID3D11Device, sink: Arc<VideoSink>, cfg: &Config) -> Result<SinkGpu> {
        let fps = cfg.canvas.fps;
        unsafe {
            let desc = D3D11_VIDEO_PROCESSOR_CONTENT_DESC {
                InputFrameFormat: D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE,
                InputFrameRate: DXGI_RATIONAL { Numerator: fps, Denominator: 1 },
                InputWidth: cfg.canvas.width,
                InputHeight: cfg.canvas.height,
                OutputFrameRate: DXGI_RATIONAL { Numerator: fps, Denominator: 1 },
                OutputWidth: sink.width,
                OutputHeight: sink.height,
                Usage: D3D11_VIDEO_USAGE_PLAYBACK_NORMAL,
            };
            let en = self.vdev.CreateVideoProcessorEnumerator(&desc)?;
            let vp = self.vdev.CreateVideoProcessor(&en, 0)?;
            self.vctx.VideoProcessorSetStreamColorSpace1(&vp, 0, DXGI_COLOR_SPACE_RGB_FULL_G22_NONE_P709);
            self.vctx.VideoProcessorSetOutputColorSpace1(&vp, DXGI_COLOR_SPACE_YCBCR_STUDIO_G22_LEFT_P709);
            self.vctx.VideoProcessorSetStreamAutoProcessingMode(&vp, 0, false);
            let ivd = D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC {
                FourCC: 0,
                ViewDimension: D3D11_VPIV_DIMENSION_TEXTURE2D,
                Anonymous: D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC_0 { Texture2D: D3D11_TEX2D_VPIV { MipSlice: 0, ArraySlice: 0 } },
            };
            let mut in_view = None;
            self.vdev.CreateVideoProcessorInputView(&self.canvas, &en, &ivd, Some(&mut in_view))?;
            let sw = if sink.input == InputKind::D3D11 {
                None
            } else {
                let rt = gpu::create_texture(
                    dev,
                    &gpu::texture_desc(sink.width, sink.height, DXGI_FORMAT_NV12, D3D11_BIND_RENDER_TARGET, D3D11_RESOURCE_MISC_FLAG(0)),
                )?;
                let ovd = D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC {
                    ViewDimension: D3D11_VPOV_DIMENSION_TEXTURE2D,
                    Anonymous: D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC_0 { Texture2D: D3D11_TEX2D_VPOV { MipSlice: 0 } },
                };
                let mut ov = None;
                self.vdev.CreateVideoProcessorOutputView(&rt, &en, &ovd, Some(&mut ov))?;
                let mut staging = Vec::new();
                for _ in 0..3 {
                    let mut d = gpu::texture_desc(sink.width, sink.height, DXGI_FORMAT_NV12, D3D11_BIND_FLAG(0), D3D11_RESOURCE_MISC_FLAG(0));
                    d.Usage = D3D11_USAGE_STAGING;
                    d.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;
                    staging.push(gpu::create_texture(dev, &d)?);
                }
                Some(SwPath { rt, out_view: ov.unwrap(), staging, pending: vec![None; 3], idx: 0 })
            };
            Ok(SinkGpu { sink, vp, en, in_view: in_view.unwrap(), hw_views: HashMap::new(), sw })
        }
    }

    fn blt(&self, g: &SinkGpu, ov: &ID3D11VideoProcessorOutputView) -> Result<()> {
        let stream = D3D11_VIDEO_PROCESSOR_STREAM {
            Enable: true.into(),
            pInputSurface: std::mem::ManuallyDrop::new(Some(g.in_view.clone())),
            ..Default::default()
        };
        let streams = [stream];
        let r = unsafe { self.vctx.VideoProcessorBlt(&g.vp, ov, 0, &streams) };
        let [s] = streams;
        drop(std::mem::ManuallyDrop::into_inner(s.pInputSurface));
        r?;
        Ok(())
    }

    fn feed(&mut self, key: usize, ctx: &ID3D11DeviceContext, n: i64) -> Result<()> {
        let input = self.sinks[&key].sink.input;
        if input == InputKind::D3D11 {
            let g = self.sinks.get(&key).unwrap();
            let sink = g.sink.clone();
            unsafe {
                let frame = OwnedFrame(sys::av_frame_alloc());
                let r = sys::av_hwframe_get_buffer(sink.hw_frames, frame.0, 0);
                if r < 0 {
                    sink.shared.dropped_queue_full.fetch_add(1, Ordering::Relaxed);
                    return Ok(());
                }
                let tex_ptr = (*frame.0).data[0] as *mut std::ffi::c_void;
                let slice = (*frame.0).data[1] as usize as u32;
                let tex = ID3D11Texture2D::from_raw_borrowed(&tex_ptr).ok_or_else(|| anyhow::anyhow!("null hw texture"))?;
                let vk = (tex_ptr as usize, slice);
                if !self.sinks[&key].hw_views.contains_key(&vk) {
                    let mut td = D3D11_TEXTURE2D_DESC::default();
                    tex.GetDesc(&mut td);
                    let ovd = if td.ArraySize > 1 {
                        D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC {
                            ViewDimension: D3D11_VPOV_DIMENSION_TEXTURE2DARRAY,
                            Anonymous: D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC_0 {
                                Texture2DArray: D3D11_TEX2D_ARRAY_VPOV { MipSlice: 0, FirstArraySlice: slice, ArraySize: 1 },
                            },
                        }
                    } else {
                        D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC {
                            ViewDimension: D3D11_VPOV_DIMENSION_TEXTURE2D,
                            Anonymous: D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC_0 { Texture2D: D3D11_TEX2D_VPOV { MipSlice: 0 } },
                        }
                    };
                    let mut ov = None;
                    self.vdev.CreateVideoProcessorOutputView(tex, &self.sinks[&key].en, &ovd, Some(&mut ov))?;
                    self.sinks.get_mut(&key).unwrap().hw_views.insert(vk, ov.unwrap());
                }
                let g = &self.sinks[&key];
                self.blt(g, &g.hw_views[&vk])?;
                (*frame.0).pts = n;
                if sink.tx.try_send(frame).is_err() {
                    sink.shared.dropped_queue_full.fetch_add(1, Ordering::Relaxed);
                }
            }
            return Ok(());
        }

        // System-memory path: NV12 render target -> staging ring -> AVFrame (2-frame delay).
        {
            let g = &self.sinks[&key];
            let sw = g.sw.as_ref().unwrap();
            self.blt(g, &sw.out_view)?;
            unsafe { ctx.CopyResource(&sw.staging[sw.idx], &sw.rt) };
        }
        let g = self.sinks.get_mut(&key).unwrap();
        let sink = g.sink.clone();
        let sw = g.sw.as_mut().unwrap();
        sw.pending[sw.idx] = Some(n);
        let oldest = (sw.idx + 1) % sw.staging.len();
        sw.idx = oldest;
        let Some(pts) = sw.pending[oldest].take() else { return Ok(()) };
        if sink.tx.is_full() {
            sink.shared.dropped_queue_full.fetch_add(1, Ordering::Relaxed);
            return Ok(());
        }
        unsafe {
            let mut m = D3D11_MAPPED_SUBRESOURCE::default();
            ctx.Map(&sw.staging[oldest], 0, D3D11_MAP_READ, 0, Some(&mut m))?;
            let frame = OwnedFrame(sys::av_frame_alloc());
            let (w, h) = (sink.width as usize, sink.height as usize);
            (*frame.0).width = w as i32;
            (*frame.0).height = h as i32;
            (*frame.0).format = if input == InputKind::Nv12 { sys::AVPixelFormat::AV_PIX_FMT_NV12 } else { sys::AVPixelFormat::AV_PIX_FMT_YUV420P } as i32;
            if sys::av_frame_get_buffer(frame.0, 0) < 0 {
                ctx.Unmap(&sw.staging[oldest], 0);
                anyhow::bail!("av_frame_get_buffer");
            }
            let src = m.pData as *const u8;
            let pitch = m.RowPitch as usize;
            let f = &*frame.0;
            for y in 0..h {
                std::ptr::copy_nonoverlapping(src.add(y * pitch), f.data[0].add(y * f.linesize[0] as usize), w);
            }
            let uv = src.add(pitch * h);
            for y in 0..h / 2 {
                let row = uv.add(y * pitch);
                if input == InputKind::Nv12 {
                    std::ptr::copy_nonoverlapping(row, f.data[1].add(y * f.linesize[1] as usize), w);
                } else {
                    let u = f.data[1].add(y * f.linesize[1] as usize);
                    let v = f.data[2].add(y * f.linesize[2] as usize);
                    for x in 0..w / 2 {
                        *u.add(x) = *row.add(2 * x);
                        *v.add(x) = *row.add(2 * x + 1);
                    }
                }
            }
            ctx.Unmap(&sw.staging[oldest], 0);
            (*frame.0).pts = pts;
            if sink.tx.try_send(frame).is_err() {
                sink.shared.dropped_queue_full.fetch_add(1, Ordering::Relaxed);
            }
        }
        Ok(())
    }
}
