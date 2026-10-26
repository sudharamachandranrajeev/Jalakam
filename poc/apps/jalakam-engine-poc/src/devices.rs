//! `devices` command: enumerate capture devices, capture methods and encoders.

use serde_json::{json, Value};

use crate::{audio, camera, display, encoder, ff, gpu};

pub fn report(adapter_pref: &str) -> Value {
    let adapters: Vec<Value> = gpu::list_adapters()
        .unwrap_or_default()
        .iter()
        .map(|a| {
            json!({
                "index": a.index, "name": a.name, "vendor": gpu::vendor_name(a.vendor_id),
                "dedicated_mb": a.dedicated_mb, "software": a.software,
            })
        })
        .collect();
    let gpu = gpu::Gpu::create(adapter_pref);
    let encoders: Vec<Value> = match &gpu {
        Ok(g) => ["h264_nvenc", "h264_amf", "h264_qsv", "h264_mf", "libopenh264"]
            .iter()
            .map(|n| match encoder::probe(n, g) {
                Ok(d) => json!({ "name": n, "available": true, "detail": d }),
                Err(e) => json!({ "name": n, "available": false, "detail": format!("{e:#}") }),
            })
            .collect(),
        Err(e) => vec![json!({ "error": format!("GPU init failed: {e:#}") })],
    };
    json!({
        "ffmpeg": { "version": ff::version_string(), "license": ff::license_string() },
        "adapters": adapters,
        "engine_adapter": gpu.as_ref().map(|g| g.adapter_name.clone()).unwrap_or_default(),
        "capture_methods": {
            "display": {
                "api": "Windows.Graphics.Capture",
                "supported": display::wgc_supported(),
                "borderless_supported": display::wgc_borderless_supported(),
            },
            "camera": { "api": "Media Foundation source reader" },
            "audio": { "api": "WASAPI shared, event-driven" },
        },
        "monitors": display::list_monitors(),
        "cameras": camera::list_cameras(),
        "audio_endpoints": audio::list_endpoints(),
        "encoders": encoders,
    })
}
