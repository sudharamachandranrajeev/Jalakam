//! Engine orchestration: owns every component and its lifecycle.
//! Control requests arrive on pipe threads; long operations (thread joins)
//! run outside the component locks so stats keep flowing.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

use anyhow::Result;
use crossbeam_channel::Sender;
use jkp_proto::{AudioStats, EncoderStats, OutputStats, Request, Response, SourceStats, State, StatsSnapshot, PROTOCOL_VERSION};
use parking_lot::Mutex;
use serde_json::json;

use crate::audio::{AudioBlock, AudioCapture};
use crate::camera::Camera;
use crate::clock::Clock;
use crate::config::{Config, VideoOutputConfig};
use crate::encoder::VideoEncoder;
use crate::gpu::Gpu;
use crate::mixer::Mixer;
use crate::output::{Output, OutputSpec};
use crate::procstats::ProcSampler;
use crate::render::{RenderShared, Renderer};

struct Pipeline {
    enc: VideoEncoder,
    out: Output,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Stream,
    Record,
}

pub struct Engine {
    pub cfg: Config,
    gpu: Arc<Gpu>,
    clock: Clock,
    started: Instant,
    render_sh: Arc<RenderShared>,
    renderer: Mutex<Option<Renderer>>,
    mixer: Mutex<Option<Mixer>>,
    audio_tx: Sender<AudioBlock>,
    mic: Mutex<Option<AudioCapture>>,
    camera: Mutex<Option<Camera>>,
    stream: Mutex<Option<Pipeline>>,
    record: Mutex<Option<Pipeline>>,
    /// Final stats of the last stopped pipeline of each kind (for reporting).
    last_stopped: Mutex<Vec<(EncoderStats, OutputStats)>>,
    /// Serialises start/stop requests so two clients cannot race a pipeline.
    op_lock: Mutex<()>,
    proc: Mutex<ProcSampler>,
    pub shutdown: AtomicBool,
    pub last_record_path: Mutex<Option<String>>,
}

impl Engine {
    pub fn start(cfg: Config) -> Result<Arc<Self>> {
        let gpu = Arc::new(Gpu::create(&cfg.adapter)?);
        crate::log!("adapter: {} (vendor 0x{:04x})", gpu.adapter_name, gpu.vendor_id);
        let clock = Clock::start();
        let (audio_tx, audio_rx) = crossbeam_channel::bounded::<AudioBlock>(64); // ~640 ms of 10 ms blocks
        let mixer = Mixer::start(audio_rx, clock, cfg.audio.bitrate_kbps)?;
        let render_sh = Arc::new(RenderShared::default());
        let camera = cfg
            .sources
            .camera
            .clone()
            .map(|sel| Camera::start(sel, cfg.sources.camera_width, cfg.sources.camera_height, cfg.sources.camera_fps));
        *render_sh.camera.lock() = camera.as_ref().map(|c| c.shared.clone());
        let renderer = Renderer::start(cfg.clone(), gpu.clone(), clock, render_sh.clone())?;
        let mic = AudioCapture::start(cfg.audio.mic.clone(), audio_tx.clone());
        std::fs::create_dir_all(&cfg.out_dir)?;
        Ok(Arc::new(Self {
            cfg,
            gpu,
            clock,
            started: Instant::now(),
            render_sh,
            renderer: Mutex::new(Some(renderer)),
            mixer: Mutex::new(Some(mixer)),
            audio_tx,
            mic: Mutex::new(Some(mic)),
            camera: Mutex::new(camera),
            stream: Mutex::new(None),
            record: Mutex::new(None),
            last_stopped: Mutex::new(Vec::new()),
            op_lock: Mutex::new(()),
            proc: Mutex::new(ProcSampler::new()),
            shutdown: AtomicBool::new(false),
            last_record_path: Mutex::new(None),
        }))
    }

    fn slot(&self, k: Kind) -> &Mutex<Option<Pipeline>> {
        match k {
            Kind::Stream => &self.stream,
            Kind::Record => &self.record,
        }
    }

    pub fn start_output(&self, k: Kind) -> Result<serde_json::Value> {
        let _op = self.op_lock.lock();
        if self.slot(k).lock().is_some() {
            anyhow::bail!("OUT.ALREADY_ACTIVE: {k:?} is already running");
        }
        let oc: &VideoOutputConfig = match k {
            Kind::Stream => &self.cfg.stream,
            Kind::Record => &self.cfg.record,
        };
        let label = match k {
            Kind::Stream => "stream",
            Kind::Record => "record",
        };
        let target = if k == Kind::Record {
            let ts = chrono_like_ts();
            let p = oc.target.replace("{ts}", &ts);
            let p = std::path::Path::new(&p);
            let abs = if p.is_absolute() { p.to_path_buf() } else { std::env::current_dir()?.join(p) };
            if let Some(d) = abs.parent() {
                std::fs::create_dir_all(d)?;
            }
            let s = abs.to_string_lossy().into_owned();
            *self.last_record_path.lock() = Some(s.clone());
            s
        } else {
            oc.target.clone()
        };
        let enc = VideoEncoder::start(label, oc, self.cfg.canvas.fps, &self.gpu)?;
        let (vpar, apar) = {
            let m = self.mixer.lock();
            (enc.codecpar.clone(), m.as_ref().unwrap().codecpar.clone())
        };
        let packets_per_s = self.cfg.canvas.fps as usize + 48; // video + AAC packets
        let spec = OutputSpec {
            name: label.into(),
            target: target.clone(),
            format: oc.format.clone(),
            network: k == Kind::Stream,
            video: vpar,
            audio: apar,
            queue_packets: oc.queue_s.max(1) as usize * packets_per_s,
        };
        let out = {
            let m = self.mixer.lock();
            Output::start(spec, &enc.shared.bus, &m.as_ref().unwrap().shared.bus)
        };
        self.render_sh.sinks.lock().push(enc.sink.clone());
        let info = json!({ "encoder": enc.shared.opened.lock().clone(), "target": crate::output::redact(&target) });
        *self.slot(k).lock() = Some(Pipeline { enc, out });
        crate::log!("{label} started: {info}");
        Ok(info)
    }

    pub fn stop_output(&self, k: Kind) -> Result<serde_json::Value> {
        let _op = self.op_lock.lock();
        let Some(p) = self.slot(k).lock().take() else {
            anyhow::bail!("OUT.NOT_ACTIVE: {k:?} is not running");
        };
        let t = Instant::now();
        // 1. stop feeding frames  2. flush encoder into the output queue  3. drain + trailer
        let sink_ptr = Arc::as_ptr(&p.enc.sink);
        self.render_sh.sinks.lock().retain(|s| Arc::as_ptr(s) != sink_ptr);
        let enc_stats = p.enc.stats();
        let enc_shared = p.enc.shared.clone();
        p.enc.stop();
        let out_stats_before = {
            let m = self.mixer.lock();
            p.out.stop(&enc_shared.bus, &m.as_ref().unwrap().shared.bus)
        };
        let mut last = self.last_stopped.lock();
        last.retain(|(e, _)| e.name != enc_stats.name);
        last.push((enc_stats, out_stats_before));
        let ms = t.elapsed().as_millis();
        crate::log!("{k:?} stopped in {ms} ms");
        let path = if k == Kind::Record { self.last_record_path.lock().clone() } else { None };
        Ok(json!({ "stopped_in_ms": ms, "path": path }))
    }

    pub fn select_camera(&self, device: Option<String>) -> serde_json::Value {
        let _op = self.op_lock.lock();
        let old = self.camera.lock().take();
        *self.render_sh.camera.lock() = None;
        if let Some(c) = old {
            c.stop();
        }
        let s = &self.cfg.sources;
        let cam = device.clone().map(|sel| Camera::start(sel, s.camera_width, s.camera_height, s.camera_fps));
        *self.render_sh.camera.lock() = cam.as_ref().map(|c| c.shared.clone());
        *self.camera.lock() = cam;
        json!({ "camera": device })
    }

    pub fn select_mic(&self, device: Option<String>) -> serde_json::Value {
        let _op = self.op_lock.lock();
        if let Some(m) = self.mic.lock().take() {
            m.stop();
        }
        *self.mic.lock() = Some(AudioCapture::start(device.clone(), self.audio_tx.clone()));
        json!({ "mic": device })
    }

    /// Tick counter for the stall watchdog (lock-free on purpose).
    pub fn render_shared(&self) -> Arc<RenderShared> {
        self.render_sh.clone()
    }

    pub fn snapshot(&self) -> StatsSnapshot {
        let r = &self.render_sh;
        let ticks = r.ticks.load(Ordering::Relaxed);
        let gpu_n = r.gpu_samples.load(Ordering::Relaxed).max(1);
        // One lock per ring and statement: guards in a struct literal live
        // until the end of the statement, and these mutexes are not reentrant.
        let (comp_mean, comp_p95) = {
            let g = r.composite_us.lock();
            (g.mean_ms(), g.pct_ms(95.0))
        };
        let (conv_mean, conv_p95) = {
            let g = r.convert_us.lock();
            (g.mean_ms(), g.pct_ms(95.0))
        };
        let tick_p99 = r.tick_us.lock().pct_ms(99.0);
        let gpu_p95 = r.gpu_us.lock().pct_ms(95.0);
        let jitter_p99 = r.jitter_us.lock().pct_ms(99.0);
        let render = jkp_proto::RenderStats {
            fps_target: self.cfg.canvas.fps,
            ticks,
            lagged: r.lagged.load(Ordering::Relaxed),
            tick_ms_avg: r.tick_ns_total.load(Ordering::Relaxed) as f64 / ticks.max(1) as f64 / 1e6,
            tick_ms_max: r.tick_ns_max.load(Ordering::Relaxed) as f64 / 1e6,
            gpu_ms_avg: r.gpu_ns_total.load(Ordering::Relaxed) as f64 / gpu_n as f64 / 1e6,
            tick_ms_p99_10s: tick_p99,
            gpu_ms_p95_10s: gpu_p95,
            gpu_composite_ms_mean_10s: comp_mean,
            gpu_composite_ms_p95_10s: comp_p95,
            gpu_convert_ms_mean_10s: conv_mean,
            gpu_convert_ms_p95_10s: conv_p95,
            jitter_ms_p99_10s: jitter_p99,
            preview_skipped: r.preview_skipped.load(Ordering::Relaxed),
        };
        let display = SourceStats {
            name: format!("display {:?}", self.cfg.sources.display),
            status: r.display_status.get(),
            frames: r.display_frames.load(Ordering::Relaxed),
            format: r.display_format.lock().clone(),
        };
        let camera = match self.camera.lock().as_ref() {
            Some(c) => SourceStats {
                name: c.shared.name.lock().clone(),
                status: c.shared.status.get(),
                frames: c.shared.frames.load(Ordering::Relaxed),
                format: c.shared.format.lock().clone(),
            },
            None => SourceStats { name: "none".into(), ..Default::default() },
        };
        let (mixer_stats, underruns, input_active) = match self.mixer.lock().as_ref() {
            Some(m) => (Some(m.stats()), m.shared.underruns.load(Ordering::Relaxed), m.shared.input_active.load(Ordering::Relaxed)),
            None => (None, 0, false),
        };
        let audio = match self.mic.lock().as_ref() {
            Some(m) => {
                let peak = f32::from_bits(m.shared.peak.swap(0, Ordering::Relaxed));
                AudioStats {
                    status: m.shared.status.get(),
                    device: m.shared.device.lock().clone(),
                    format: m.shared.format.lock().clone(),
                    blocks_captured: m.shared.blocks.load(Ordering::Relaxed),
                    underruns,
                    overruns: m.shared.overruns.load(Ordering::Relaxed),
                    peak_dbfs: if peak > 0.0 { 20.0 * peak.log10() } else { -120.0 },
                    drift_ms: m.shared.drift_us.load(Ordering::Relaxed) as i64 as f64 / 1000.0,
                }
            }
            None => AudioStats::default(),
        };
        let _ = input_active;
        let mut encoders = Vec::new();
        let mut outputs = Vec::new();
        for slot in [&self.stream, &self.record] {
            if let Some(p) = slot.lock().as_ref() {
                encoders.push(p.enc.stats());
                outputs.push(p.out.stats());
            }
        }
        for (e, o) in self.last_stopped.lock().iter() {
            if !encoders.iter().any(|x| x.name == e.name) {
                let mut e = e.clone();
                e.status.state = State::Idle;
                let mut o = o.clone();
                o.status.state = State::Idle;
                encoders.push(e);
                outputs.push(o);
            }
        }
        encoders.extend(mixer_stats);
        StatsSnapshot {
            uptime_s: self.started.elapsed().as_secs_f64(),
            adapter: self.gpu.adapter_name.clone(),
            render,
            display,
            camera,
            audio,
            encoders,
            outputs,
            process: self.proc.lock().sample(),
        }
    }

    pub fn handle(&self, req: Request) -> Response {
        let r: Result<serde_json::Value> = match req {
            Request::Hello { version } => Ok(json!({ "version": PROTOCOL_VERSION, "client_version": version, "pid": std::process::id() })),
            Request::Stats => serde_json::to_value(self.snapshot()).map_err(Into::into),
            Request::StreamStart => self.start_output(Kind::Stream),
            Request::StreamStop => self.stop_output(Kind::Stream),
            Request::RecordStart => self.start_output(Kind::Record),
            Request::RecordStop => self.stop_output(Kind::Record),
            Request::CameraSelect { device } => Ok(self.select_camera(device)),
            Request::MicSelect { device } => Ok(self.select_mic(device)),
            Request::Shutdown => {
                self.shutdown.store(true, Ordering::SeqCst);
                Ok(json!({ "shutting_down": true }))
            }
        };
        match r {
            Ok(result) => Response::Ok { result },
            Err(e) => {
                let msg = format!("{e:#}");
                let code = msg.split(':').next().filter(|c| c.contains('.') && c.chars().all(|ch| ch.is_ascii_uppercase() || ch == '.' || ch == '_'));
                Response::Error { code: code.unwrap_or("ENGINE.ERROR").to_string(), message: msg }
            }
        }
    }

    /// Ordered teardown. Returns the time taken.
    pub fn shutdown_all(&self) -> std::time::Duration {
        let t = Instant::now();
        for k in [Kind::Stream, Kind::Record] {
            if self.slot(k).lock().is_some() {
                let _ = self.stop_output(k);
            }
        }
        if let Some(c) = self.camera.lock().take() {
            *self.render_sh.camera.lock() = None;
            c.stop();
            crate::log!("shutdown: camera stopped at {} ms", t.elapsed().as_millis());
        }
        if let Some(m) = self.mic.lock().take() {
            m.stop();
            crate::log!("shutdown: mic stopped at {} ms", t.elapsed().as_millis());
        }
        if let Some(r) = self.renderer.lock().take() {
            r.stop();
            crate::log!("shutdown: renderer stopped at {} ms", t.elapsed().as_millis());
        }
        if let Some(m) = self.mixer.lock().take() {
            m.stop();
            crate::log!("shutdown: mixer stopped at {} ms", t.elapsed().as_millis());
        }
        let _ = &self.clock;
        t.elapsed()
    }
}

fn chrono_like_ts() -> String {
    let st = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
    format!("{:04}{:02}{:02}-{:02}{:02}{:02}", st.wYear, st.wMonth, st.wDay, st.wHour, st.wMinute, st.wSecond)
}
