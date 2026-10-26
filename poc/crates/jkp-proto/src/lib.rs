//! Control protocol and stats types shared by the POC engine, preview and CLI.
//!
//! Transport (ADR-0008): one JSON object per line over the named pipe
//! [`PIPE_NAME`]. Every request carries an `id`; the response echoes it.
//! Stats are polled with [`Request::Stats`].

use serde::{Deserialize, Serialize};

pub const PIPE_NAME: &str = r"\\.\pipe\jalakam-poc-engine";
/// Named file mapping holding [`PreviewMeta`] (see preview ring in ADR-0008).
pub const PREVIEW_META_NAME: &str = "Local\\JalakamPocPreviewMeta";
pub const PREVIEW_SLOTS: usize = 3;
pub const PROTOCOL_VERSION: u32 = 1;

pub fn preview_texture_name(slot: usize) -> String {
    format!("Local\\JalakamPocPreview{slot}")
}

/// Layout of the shared preview metadata section. Written by the engine with
/// a sequence-lock: `seq` is odd while the engine updates the fields.
#[repr(C)]
pub struct PreviewMeta {
    pub seq: std::sync::atomic::AtomicU64,
    pub latest_slot: std::sync::atomic::AtomicU32,
    pub width: std::sync::atomic::AtomicU32,
    pub height: std::sync::atomic::AtomicU32,
    pub adapter_luid_low: std::sync::atomic::AtomicU32,
    pub adapter_luid_high: std::sync::atomic::AtomicI32,
    pub frame_number: std::sync::atomic::AtomicU64,
    /// Bumped whenever the engine (re)creates the shared textures.
    pub generation: std::sync::atomic::AtomicU32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum Request {
    Hello { version: u32 },
    Stats,
    StreamStart,
    StreamStop,
    RecordStart,
    RecordStop,
    /// Select a camera by name substring or index ("0", "1", ...). `None` removes the camera.
    CameraSelect { device: Option<String> },
    /// Select a microphone by endpoint name substring. `None` = default capture endpoint.
    MicSelect { device: Option<String> },
    Shutdown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Envelope<T> {
    pub id: u64,
    #[serde(flatten)]
    pub body: T,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum Response {
    Ok { result: serde_json::Value },
    Error { code: String, message: String },
}

/// Lifecycle of a source, output or encoder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Idle,
    Starting,
    Active,
    /// Running with a problem (placeholder frames, silence, reconnecting).
    Degraded,
    Reconnecting,
    Stopping,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComponentStatus {
    pub state: State,
    /// Stable namespaced error code, e.g. `CAP.CAMERA.NOT_FOUND`.
    pub code: Option<String>,
    pub detail: Option<String>,
}

impl Default for ComponentStatus {
    fn default() -> Self {
        Self { state: State::Idle, code: None, detail: None }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SourceStats {
    pub name: String,
    pub status: ComponentStatus,
    pub frames: u64,
    /// Width x height x fps or rate x channels, as negotiated.
    pub format: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AudioStats {
    pub status: ComponentStatus,
    pub device: String,
    pub format: String,
    pub blocks_captured: u64,
    /// Mixer ticks that found no captured audio and produced silence.
    pub underruns: u64,
    /// Captured blocks discarded because the bounded ring was full.
    pub overruns: u64,
    pub peak_dbfs: f32,
    /// Device clock minus QPC clock, milliseconds (positive: device ahead).
    pub drift_ms: f64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RenderStats {
    pub fps_target: u32,
    pub ticks: u64,
    /// Ticks that ran later than one frame interval (render lag).
    pub lagged: u64,
    /// Average and max CPU time per tick, ms.
    pub tick_ms_avg: f64,
    pub tick_ms_max: f64,
    /// GPU time for the composite pass (timestamp queries), ms.
    pub gpu_ms_avg: f64,
    /// Percentiles over the last 10 s (600 ticks at 60 fps); the budgets in
    /// performance-budgets.md are stated this way. Tick time is wall time on
    /// the render thread; jitter is wake-up lateness against the QPC schedule.
    #[serde(default)]
    pub tick_ms_p99_10s: f64,
    #[serde(default)]
    pub gpu_ms_p95_10s: f64,
    /// GPU time of the composite pass plus preview downscale only (timestamps
    /// placed after CPU uploads and D2D text, so no CPU bubbles inside).
    #[serde(default)]
    pub gpu_composite_ms_mean_10s: f64,
    #[serde(default)]
    pub gpu_composite_ms_p95_10s: f64,
    /// GPU time of the encoder colour conversions (VideoProcessorBlt to NV12;
    /// on the software path this span also contains the staging readback).
    #[serde(default)]
    pub gpu_convert_ms_mean_10s: f64,
    #[serde(default)]
    pub gpu_convert_ms_p95_10s: f64,
    #[serde(default)]
    pub jitter_ms_p99_10s: f64,
    pub preview_skipped: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EncoderStats {
    pub name: String,
    pub codec: String,
    /// True only if the encoder that opened is a hardware encoder AND it was
    /// fed GPU frames. See also `ProcessStats::gpu_video_encode_pct`.
    pub hardware: bool,
    pub zero_copy: bool,
    pub settings: String,
    pub status: ComponentStatus,
    pub frames_in: u64,
    pub packets_out: u64,
    /// Frames dropped because the encoder input queue was full.
    pub dropped_queue_full: u64,
    pub encode_ms_avg: f64,
    pub queue_depth: usize,
    pub errors: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OutputStats {
    pub name: String,
    pub target: String,
    pub status: ComponentStatus,
    pub bytes: u64,
    pub kbps: f64,
    pub video_packets: u64,
    pub audio_packets: u64,
    /// Packets dropped by the output (queue full, waiting for keyframe, reconnecting).
    pub dropped_packets: u64,
    pub reconnects: u64,
    pub uptime_s: f64,
    pub queue_depth: usize,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProcessStats {
    pub cpu_pct_of_one_core: f64,
    pub cpu_pct_of_machine: f64,
    pub working_set_mb: f64,
    pub private_mb: f64,
    pub handles: u32,
    pub threads: u32,
    /// GPU engine utilisation of this process from the "GPU Engine" PDH counters.
    pub gpu_3d_pct: f64,
    pub gpu_video_encode_pct: f64,
    /// VideoProcessorBlt (RGB to NV12) runs on this engine, not on 3D.
    #[serde(default)]
    pub gpu_video_processing_pct: f64,
    #[serde(default)]
    pub gpu_copy_pct: f64,
    pub gpu_dedicated_mb: f64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StatsSnapshot {
    pub uptime_s: f64,
    pub adapter: String,
    pub render: RenderStats,
    pub display: SourceStats,
    pub camera: SourceStats,
    pub audio: AudioStats,
    pub encoders: Vec<EncoderStats>,
    pub outputs: Vec<OutputStats>,
    pub process: ProcessStats,
}
