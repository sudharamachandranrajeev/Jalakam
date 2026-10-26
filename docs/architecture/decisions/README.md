# Architecture Decision Records

ADR 0001 (Phase 0 product decisions D1–D5) lives in `docs/decisions/0001-phase0-decisions.md`. Phase 2 architecture ADRs start at ADR-0002.

| ADR | Title | Status | Validated by |
|---|---|---|---|
| [ADR-0002](ADR-0002-rust-core.md) | Rust for the core, engine and workers | Accepted | POC-1A |
| [ADR-0003](ADR-0003-tauri-ui.md) | Tauri 2 + React UI, native preview surface, Flutter fallback | Accepted, conditional | POC-1C |
| [ADR-0004](ADR-0004-custom-pipeline-ffmpeg.md) | Custom real-time pipeline + FFmpeg (LGPL) | Accepted | POC-1A |
| [ADR-0005](ADR-0005-multi-process.md) | Controlled multi-process architecture | Accepted | FI tests |
| [ADR-0006](ADR-0006-d3d11-compositor.md) | D3D11 compositor on a single render thread | Accepted | POC-1A |
| [ADR-0007](ADR-0007-capture-apis.md) | Capture APIs: WGC, MF, WASAPI, CEF OSR | Accepted | POC-1A, POC-2 |
| [ADR-0008](ADR-0008-ipc.md) | Named pipes + JSON-RPC; shared memory and shared textures | Accepted | POC-1C |
| [ADR-0009](ADR-0009-desired-state-scenes.md) | Desired-state scene model with reconciliation | Accepted | Property tests |
| [ADR-0010](ADR-0010-local-first-ai.md) | Local-first AI; optional cloud LLM for metadata | Accepted | POC-1B |
| [ADR-0011](ADR-0011-background-governance.md) | Background governance: governor, Job Objects, GPU priority | Accepted | POC-1B |
| [ADR-0012](ADR-0012-persistence.md) | SQLite WAL + atomic JSON documents | Accepted | Power-cut test |
| [ADR-0013](ADR-0013-nondestructive-deterministic-export.md) | Non-destructive editing, deterministic export manifests | Accepted | Determinism test |
| [ADR-0014](ADR-0014-output-reliability.md) | Encoder graph, bounded queues, reconnect state machine | Accepted | POC-1A |
| [ADR-0015](ADR-0015-observability.md) | Logs, trace IDs, crash reporting, opt-in telemetry | Accepted | CI crash test |
| [ADR-0016](ADR-0016-editor-preview.md) | Editor preview: HTML video + exact paused frames | Accepted, pending | POC-4 |

## Selected stack (summary)

| Layer | Choice |
|---|---|
| Platform | Windows 10 22H2+ / 11, x64; NVIDIA-first, AMD/Intel/MF fallback |
| Language | Rust (C++ only in thin C-ABI shims) |
| UI | Tauri 2 + React/TypeScript in WebView2; native D3D11 child-window preview |
| Processes | App Host, Media Engine, CEF browser, AI/render workers, Crashpad handler |
| Capture | WGC (DXGI duplication fallback), Media Foundation, WASAPI (+ process loopback), CEF OSR |
| Compositor | Direct3D 11, single render thread, QPC clock |
| Encode / mux | FFmpeg LGPL shared: NVENC (D3D11 zero-copy), AMF, QSV, MF H.264; MF AAC; FLV/RTMP, MKV, MP4 |
| IPC | Named pipes + JSON-RPC; shared-memory rings; DXGI shared NT-handle textures |
| Persistence | SQLite WAL (records) + atomic versioned JSON (documents) |
| AI | whisper.cpp, Silero VAD on ONNX Runtime, local scoring; opt-in BYO-key LLM (Anthropic adapter) |
| Background control | Governor + Job Objects (CPU-rate and memory caps) + GPU thread priority |
| Observability | `tracing` JSON logs, trace IDs, Crashpad minidumps, opt-in telemetry |
| Security | User-only pipe ACLs, no TCP listeners, Credential Manager, signed updates, sandboxed CEF, isolated parsers |

## Proof-of-concept plan (before any product scaffolding)

**POC-1 "spine spike"** is the smallest POC that validates the riskiest decisions. Owner: Opus (high effort). Estimate: 3–4 weeks.

- **1A Engine spine** (ADR-0002/0004/0006/0007/0014):
  - WGC display + MF camera → D3D11, two canvases (1080p60 + 1080×1920);
  - `h264_nvenc` zero-copy through FFmpeg D3D11 hwframes;
  - RTMP to a local MediaMTX + MKV recording + replay save;
  - WASAPI mic + loopback, with the clapper A/V-sync test;
  - 30 s network block → reconnect;
  - forced TDR;
  - 1 h soak.

  Pass: performance-budgets.md §1–§5 on the reference machine.
- **1B Background isolation** (ADR-0010/0011): whisper.cpp at full CPU in a Job-Object-capped worker during 1A. Pass: lagged frames rise by ≤ 0.05 % and there are no audio underruns.
- **1C Preview airspace** (ADR-0003/0008): the shared preview texture shown in a Tauri child-HWND swapchain with a React UI around it. Pass: frame pacing, resize, DPI, focus and keyboard (ADR-0003). **Fail → Flutter.**

Optional, smaller follow-ups:
- **POC-2:** CEF OSR shared texture as a source.
- **POC-3:** NVENC session sharing at 3 outputs on a consumer driver.
- **POC-4:** editor exact-frame decode + export determinism hash test.
