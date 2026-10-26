//! Engine configuration (TOML). See `poc/config/baseline.toml`.

use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub canvas: CanvasConfig,
    #[serde(default)]
    pub sources: SourcesConfig,
    #[serde(default)]
    pub overlay: OverlayConfig,
    pub audio: AudioConfig,
    pub stream: VideoOutputConfig,
    pub record: VideoOutputConfig,
    #[serde(default = "default_out_dir")]
    pub out_dir: PathBuf,
    /// Preferred adapter vendor: "nvidia", "amd", "intel" or "any".
    #[serde(default = "default_adapter")]
    pub adapter: String,
}

fn default_out_dir() -> PathBuf {
    PathBuf::from("out")
}
fn default_adapter() -> String {
    "nvidia".into()
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CanvasConfig {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    #[serde(default = "default_preview_width")]
    pub preview_width: u32,
}

fn default_preview_width() -> u32 {
    960
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourcesConfig {
    /// Monitor index for display capture (WGC). `None` disables the layer.
    pub display: Option<u32>,
    /// Camera name substring or index. `None` disables the layer.
    pub camera: Option<String>,
    #[serde(default = "default_cam_w")]
    pub camera_width: u32,
    #[serde(default = "default_cam_h")]
    pub camera_height: u32,
    #[serde(default = "default_cam_fps")]
    pub camera_fps: u32,
}

fn default_cam_w() -> u32 {
    1280
}
fn default_cam_h() -> u32 {
    720
}
fn default_cam_fps() -> u32 {
    30
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OverlayConfig {
    #[serde(default = "default_overlay_text")]
    pub text: String,
}

impl Default for OverlayConfig {
    fn default() -> Self {
        Self { text: default_overlay_text() }
    }
}

fn default_overlay_text() -> String {
    "JALAKAM POC".into()
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AudioConfig {
    /// Capture endpoint name substring. `None` = default communications capture device.
    pub mic: Option<String>,
    #[serde(default = "default_audio_bitrate")]
    pub bitrate_kbps: u32,
}

fn default_audio_bitrate() -> u32 {
    160
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VideoOutputConfig {
    /// For stream: rtmp:// URL. For record: file path template; `{ts}` is replaced.
    pub target: String,
    /// FFmpeg muxer name: "flv" for RTMP, "matroska" for recording.
    pub format: String,
    /// FFmpeg encoder name ("h264_nvenc", "h264_mf", ...).
    pub encoder: String,
    /// Encoder used if `encoder` fails to open. Empty = none.
    #[serde(default)]
    pub fallback_encoder: String,
    pub width: u32,
    pub height: u32,
    pub bitrate_kbps: u32,
    #[serde(default = "default_gop_s")]
    pub keyframe_interval_s: u32,
    /// Bounded output queue length in seconds of media.
    #[serde(default = "default_queue_s")]
    pub queue_s: u32,
}

fn default_gop_s() -> u32 {
    2
}
fn default_queue_s() -> u32 {
    2
}

impl Config {
    pub fn load(path: &std::path::Path) -> anyhow::Result<Self> {
        let text = std::fs::read_to_string(path)?;
        let cfg: Config = toml::from_str(&text)?;
        anyhow::ensure!(cfg.canvas.fps > 0 && cfg.canvas.fps <= 120, "canvas.fps out of range");
        anyhow::ensure!(cfg.canvas.width % 2 == 0 && cfg.canvas.height % 2 == 0, "canvas size must be even");
        for o in [&cfg.stream, &cfg.record] {
            anyhow::ensure!(o.width % 2 == 0 && o.height % 2 == 0, "output size must be even");
        }
        Ok(cfg)
    }
}
