# MVP Recommendation

All content here is **[R]** recommendation unless cited.

## 1. MVP thesis
Prove two things with the smallest build:
1. **Streaming stability.** Jalakam can stream a 1080p60 landscape canvas and a 1080×1920 portrait canvas to two destinations, record locally, and keep a replay buffer, for 4 hours without dropped frames or A/V drift on a mid-range NVIDIA PC.
2. **AI editing value.** After (and during) that stream, Jalakam proposes highlight clips that a streamer accepts at a useful rate. They can trim them by transcript and export or publish a captioned 9:16 short in under 2 minutes per clip.

## 2. Scope

### In
| Area | MVP slice | Matrix ref |
|---|---|---|
| Platform | Windows 10 2004+ / 11, x64, NVIDIA GPU (x264 or software fallback) | F10 |
| Compositor | Scenes, layers, transforms, crop; landscape + optional portrait canvas | F01, F15 |
| Sources | Display/window capture (WGC), camera, browser source, image, text | F02, F04, F05, F06 |
| Audio | Mic + desktop + per-process capture, mixer, monitoring, gain, noise suppression | F07, F08 |
| Output | RTMP/RTMPS to up to 3 destinations, per-canvas, encoder sharing, reconnect/backoff | F11, F12 |
| Recording | Independent encoder, crash-safe MKV/fMP4, sidecar event log | F16 |
| Replay | Ring buffer of encoded packets per canvas, clip hotkey, 10–120 s | F18 |
| AI | Local live transcription; moment scoring (audio energy, transcript keywords, Twitch chat velocity); suggestions panel | F20, F21, F31◐ |
| Editor | Transcript timeline, trim, burned-in captions, fixed 9:16 crop presets, export presets | F22, F23◐, F24 |
| Publish | YouTube upload with AI-drafted title/description; each publish needs human confirmation | F25◐, F26 |
| Automation | Local WebSocket JSON-RPC: scene switch, start/stop, clip | F27◐ |
| Ops | Crash reporting, opt-in telemetry, config snapshots before upgrade | F34 |

### Out (explicitly deferred)
Game-capture hooking, Twitch Enhanced Broadcasting, cloud relay, virtual camera, VST3, macOS/Linux, Spark-style overlay generator, OBS import, TikTok/Twitch publishing, background removal, in-scene instant replay.

## 3. Milestones and exit gates
| M | Deliverable | Exit gate |
|---|---|---|
| M0 | Architecture + ADRs from approved decisions | Decisions D1–D5 signed off |
| M1 | Media spine: capture → compositor → NVENC → RTMP + record | 1 h stream at 1080p60, < 0.1 % dropped frames, A/V offset < 20 ms (clapper test) |
| M2 | Dual canvas + multistream + replay buffer | 4 h soak: 2 canvases, 2 destinations, recording and buffer all running |
| M3 | Live ASR + moment scoring | ASR does not raise dropped frames above the M2 baseline; scorer precision@10 ≥ 0.4 on our own eval VODs |
| M4 | Transcript editor + export + YouTube publish | Streamer goes from suggestion to published Short in < 2 min |
| M5 | Hardening: crash recovery, reconnect, installer, auto-update | Kill-process test loses ≤ 2 s of recording; network-drop test reconnects |

## 4. Build-model assessment (who should build what)

The question: can Claude Sonnet build Jalakam from this research, with Opus making the decisions?

**Verdict: Sonnet 5.5 can build most of Jalakam. It should not own the real-time media core.** About 70–75 % of the MVP is ordinary application engineering that Sonnet handles well at high effort: UI, scene/data model, settings, automation API, transcript editor, FFmpeg export jobs, YouTube OAuth and upload, and LLM metadata drafting. The rest is the real-time media spine (M1–M2): GPU compositor, zero-copy capture-to-encoder texture path, A/V clock and sync, encoder-sharing graph, packet ring buffer with keyframe alignment, and RTMP reconnect. Failures there are timing- and driver-dependent, hard to reproduce, and expensive to debug. Use the strongest model for design, implementation and review of that spine.

| Workstream | Model | Effort | Rationale |
|---|---|---|---|
| Architecture, ADRs, decision records, cross-module interfaces | **Opus 5.5** (`claude-opus-5-5`) | medium (per user instruction) | Decision-making role |
| Media spine: compositor, capture interop, encoder graph, A/V sync, ring buffer, RTMP/E-RTMP muxing | **Opus 5.5** | high | Highest risk (R01); concurrency, GPU interop and timing bugs |
| Code review gate for every media-spine PR and every security-relevant PR (credentials, OAuth, local API) | **Opus 5.5** | high | Catches subtle sync, race and secret-handling defects |
| UI shell, scene/layer editor, settings, mixer UI, outputs UI | **Sonnet 5.5** (`claude-sonnet-5-5`) | high | Large surface, well-trodden patterns |
| Audio DSP integration (noise suppression, gain, meters) | Sonnet 5.5 | high | Library integration, testable offline |
| ASR integration, moment scorer, eval harness | Sonnet 5.5 | high | Scorer heuristics and eval are iterative and testable on recorded VODs |
| Transcript editor, export presets, FFmpeg job runner | Sonnet 5.5 | high | Deterministic and testable |
| YouTube publish, LLM metadata, local WebSocket API | Sonnet 5.5 | high | Standard OAuth/REST/JSON-RPC; Opus reviews security |
| Installer, CI, crash reporting, config migration, docs | Sonnet 5.5 | medium | Mechanical |
| Unit tests and fixtures across modules | Sonnet 5.5 | medium | Mechanical once interfaces exist |

Escalation rule: if a Sonnet-owned task fails its acceptance test twice, or touches a media-spine interface, hand it to Opus 5.5 at high effort.

No model can complete these gates alone: M1, M2 and M5 need a human running real hardware (an NVIDIA GPU, real Twitch/YouTube ingest) and reporting measurements back.

## 5. Five decisions that need approval before architecture work

1. **D1: Platform scope.** Windows-only MVP (x64, NVIDIA-first, x264/software fallback), or cross-platform from day one?
   *Recommendation: Windows-only.* Meld's own minimum is Windows 10 2004 (S11). Cross-platform roughly doubles the capture and GPU work (F35).

2. **D2: Licence and media-library basis.** The repo is public. Pick a licence, and with it the allowed dependencies: GPL (allows libobs and x264) or permissive Apache-2.0/MIT (FFmpeg as dynamically linked LGPL, no x264, no libobs).
   *Recommendation: Apache-2.0* with an LGPL FFmpeg build, NVENC/AMF/QSV hardware encoders, and the Media Foundation H.264 encoder as the software fallback. This keeps the codebase independent of OBS internals and supports the clean-room claim.

3. **D3: Technology stack.** Options: (a) Rust core + Tauri/React UI, with the preview drawn by the core into a native child surface; (b) C++20 + Qt 6/QML; (c) Electron + native addon.
   *Recommendation: (a).* Memory safety in a long-running real-time process; UI in TypeScript/React, where Sonnet is most productive; no Qt commercial-licence questions. The main risk is preview-surface integration, so spike it in M1 week 1.

4. **D4: Where AI runs and who pays.** Options: local ASR and scoring with optional cloud LLM text calls (bring-your-own API key, or a hosted Jalakam key); or fully cloud processing.
   *Recommendation:* local ASR and scoring, so no stream media leaves the PC by default. Use a cloud LLM (Claude Haiku 5.5 or Sonnet 5.5) only for titles and descriptions: opt-in, with the user's own key in the MVP. This avoids running-cost and privacy commitments before value is proven (R02, R03).

5. **D5: Multistream and publishing scope.** Options: local multistream only or also a cloud relay; publish to YouTube only, or also apply now for TikTok Content Posting and Twitch Enhanced Broadcasting access.
   *Recommendation:* local multistream (≤ 3 outputs) with YouTube publishing in the MVP. Start the TikTok and Twitch API applications now in parallel, because approval time is outside our control (U08, U09). No cloud relay until users ask for it.
