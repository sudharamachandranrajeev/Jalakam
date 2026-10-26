# Feature Matrix

Scale: Value, Complexity and Risk each run 1 (low) to 5 (high). Differentiation: **Parity** means the reference already has it, **Better** means improved parity, **New** means not found in the reference or OBS core. MVP: ✅ in MVP, ◐ thin version in MVP, — deferred.

All ratings are researcher judgements **[R]**. Reference availability is cited in `product-landscape.md`.

| # | Feature | Value | Complexity | Risk | Key dependencies | Differentiation | MVP |
|---|---|---|---|---|---|---|---|
| F01 | GPU compositor (scenes, layers, transforms) | 5 | 5 | 5 | D3D11/12 (Win) or wgpu/Vulkan; frame clock | Parity | ✅ |
| F02 | Display / window capture | 5 | 3 | 3 | Windows Graphics Capture, DXGI duplication | Parity | ✅ |
| F03 | Game capture (hook-based) | 4 | 5 | 5 | DLL injection; anti-cheat compatibility | Parity | — (use WGC) |
| F04 | Camera capture | 5 | 3 | 2 | Media Foundation / DirectShow | Parity | ✅ |
| F05 | Browser source (overlays/alerts) | 5 | 3 | 3 | CEF or WebView2 offscreen rendering, shared textures | Parity | ✅ |
| F06 | Media / image / text layers | 4 | 2 | 2 | FFmpeg decode | Parity | ◐ image + text |
| F07 | Audio engine (capture, mix, monitor, sync) | 5 | 4 | 4 | WASAPI, process loopback, resampler, A/V clock | Parity | ✅ |
| F08 | Audio effects (noise suppression, compressor, gain) | 4 | 3 | 2 | RNNoise-class DSP | Parity | ◐ gain + NS |
| F09 | VST3 hosting | 2 | 4 | 3 | VST3 SDK licence | Parity | — |
| F10 | Hardware encode (NVENC/AMF/QSV) + x264 fallback | 5 | 4 | 4 | Vendor SDKs, driver versions | Parity | ✅ NVENC + x264 |
| F11 | RTMP/RTMPS output, reconnect | 5 | 3 | 3 | librtmp / FFmpeg / own E-RTMP muxer | Parity | ✅ |
| F12 | Local multistream with encoder sharing | 5 | 3 | 3 | F10, F11, encoder graph | Parity | ✅ (2–3 targets) |
| F13 | Twitch Enhanced Broadcasting (multitrack E-RTMP) | 3 | 4 | 4 | GetClientConfiguration, BPM SEI, aligned IDRs (S17) | Parity | — |
| F14 | Cloud relay multistream | 3 | 4 | 3 | Hosted infrastructure, running cost | Parity | — |
| F15 | Dual canvas (landscape + portrait) | 5 | 4 | 3 | F01 second render target | Parity | ✅ |
| F16 | Recording (independent encoder, crash-safe container) | 5 | 3 | 2 | F10, fMP4/MKV muxer | Parity | ✅ |
| F17 | Multitrack audio recording | 3 | 2 | 2 | F07, F16 | Parity | — |
| F18 | Replay ring buffer + clip hotkey | 5 | 3 | 3 | Encoded-packet ring buffer, keyframe alignment | Parity | ✅ |
| F19 | In-scene instant replay overlay | 3 | 3 | 2 | F18, media layer | Parity | — |
| F20 | Live transcription (local ASR) | 4 | 3 | 3 | Whisper-class model, GPU/CPU budget alongside the stream | Better | ✅ |
| F21 | AI moment detection (audio energy + transcript + chat + events) | 5 | 4 | 4 | F20, chat ingest, scoring model, eval set | **New** | ✅ |
| F22 | Transcript-based clip editor (trim, captions) | 5 | 4 | 3 | F20, timeline UI, FFmpeg render | **New** | ✅ |
| F23 | AI reframe 16:9 → 9:16 (subject tracking) | 4 | 4 | 4 | Face/person detection, smoothing | **New** | ◐ fixed crop presets |
| F24 | Export presets (Shorts/TikTok/YouTube) | 4 | 2 | 1 | FFmpeg | Better | ✅ |
| F25 | Direct publishing (YouTube, TikTok, Twitch) | 4 | 3 | 4 | Platform API approval, OAuth, quotas | **New** | ◐ YouTube only |
| F26 | AI-drafted titles, descriptions, hashtags | 3 | 2 | 2 | LLM API | **New** | ✅ |
| F27 | Local automation API (WebSocket) | 4 | 2 | 2 | JSON-RPC schema | Parity | ◐ core commands |
| F28 | Stream Deck / bot integrations | 3 | 2 | 2 | F27 | Parity | — |
| F29 | Virtual camera | 3 | 4 | 3 | Kernel/MF virtual camera (Win11 MFCreateVirtualCamera) | Parity | — |
| F30 | AI overlay generator (Spark-like) | 3 | 3 | 2 | LLM API, sandboxed browser source | Parity | — |
| F31 | Multi-chat panel | 3 | 3 | 3 | Platform chat APIs | Parity | ◐ Twitch chat read-only (feeds F21) |
| F32 | OBS scene import | 3 | 3 | 2 | OBS JSON schema (public) | Parity | — |
| F33 | Background removal / blur | 3 | 3 | 2 | Segmentation model | Parity | — |
| F34 | Crash reporting, telemetry (opt-in), config migration snapshots | 4 | 2 | 1 | Crashpad / Sentry | Parity | ✅ |
| F35 | macOS / Linux builds | 3 | 5 | 4 | Metal/Vulkan, ScreenCaptureKit, PipeWire | Better (Linux) | — |

## Reading the matrix
- The **streaming-stability spine** is F01, F02, F04, F05, F07, F10, F11, F12, F15, F16 and F18. These are the highest risk and must be proven first.
- The **AI-editing spine** is F20, F21, F22, F24 and F26, plus thin F23 and F25. This is where the product differentiates.
- The highest-risk items that deliver little MVP value (F03, F13, F14, F29, F35) are deferred.
