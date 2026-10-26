# Performance Budgets

All budgets apply to the reference machine (A-01: RTX 3060 12 GB, 6-core CPU, 16 GB RAM, NVMe, Windows 11) and the **reference load**: 1080p60 landscape + 1080×1920 portrait canvas, camera, display capture, 2 browser sources, mic + desktop audio, 2 RTMP outputs (one per canvas), landscape recording, replay rings on both canvases. Requirement IDs refer to `docs/product/product-requirements.md`.

Each budget lists how it is **measured** and how it is **enforced** at runtime or in CI. A budget that is exceeded in CI fails the build. A budget exceeded at runtime triggers the governor or a health warning (VID-008, ADR-0011).

## 1. Frame budget (60 fps = 16.67 ms)

| Stage | Budget | Measured by | Enforced by |
|---|---|---|---|
| Source capture → mailbox | ≤ 1 frame of latency; 0 ms on the render thread | QPC stamps at capture vs tick | Latest-wins mailbox (never blocks) |
| Compositor, landscape canvas | ≤ 1.0 ms GPU | D3D11 timestamp queries per pass | CI perf test (fixed scene); runtime `render.stats` |
| Compositor, portrait canvas | ≤ 1.5 ms GPU incremental (CAN-002) | Same | Same |
| Colour conversion (per encoder input) | ≤ 0.3 ms GPU | Same | Same |
| Preview downscale + shared-texture write | ≤ 0.4 ms GPU | Same | Preview frames are dropped first under load |
| **Total engine GPU per frame** | **≤ 4 ms** (leaves the GPU to the game: NFR-PERF-02) | Sum of the above | Health warning if p95 > 4 ms over 10 s |
| Render-thread CPU per tick | ≤ 2 ms | Thread CPU time | CI perf test |
| Tick jitter | p99 ≤ 1 ms | QPC deltas | Soak report |
| Lagged + skipped frames | < 0.1 % over 4 h (NFR-PERF-01) | Frame accounting (acceptance-criteria §2) | Gate G2 |

## 2. Latency budgets

| Path | Budget | Notes |
|---|---|---|
| Audio capture → mix | ≤ 20 ms (AUD-001) | 10 ms WASAPI period + 10 ms mixer quantum |
| Monitoring output | ≤ 40 ms (AUD-006) | Mixer + render endpoint buffer |
| A/V offset in output | ≤ 20 ms at start, ≤ 30 ms after 4 h (NFR-PERF-03) | Single QPC clock; drift-compensated resampling |
| Glass-to-ingest (capture → packet on wire) | ≤ 100 ms p95 | Not a requirement; tracked for regressions |
| Hotkey → action | ≤ 50 ms (ONB-005, LIB-005) | Low-level keyboard hook on its own thread |
| UI command → store revision | p99 ≤ 10 ms in-process; engine RPC p99 ≤ 10 ms | Traced per `trace_id` |
| Replay save (hotkey → file on disk) | ≤ 2 s p95 (LIB-004, SC-06) | Mux from the packet ring; no re-encode |
| Preview presentation | ≤ 2 frames behind program | Drops allowed; never back-pressures |

## 3. CPU budgets (reference load)

| Process / work | Budget | Enforced by |
|---|---|---|
| Engine total | ≤ 1.5 cores average | Soak report; regression CI |
| Each browser source (CEF) | ≤ 1 core at 1080p60 (SCN-003) | CEF frame-rate cap = canvas fps; per-source CPU reported |
| Audio (mixer + filters + AAC) | ≤ 0.3 core; noise suppression ≤ 3 % of one core per track (AUD-007) | Unit benchmarks |
| App Host + WebView2 UI | ≤ 0.5 core while live (meters/stats throttled) | Soak report |
| Stats and logging | ≤ 0.5 % of one core each (VID-008, SYS-004) | Benchmarks |
| **Background workers while live** | ≤ 2 logical cores total via Job Object CPU-rate cap; priority `IDLE`; paused on amber health | ADR-0011 governor |
| Background workers while idle | ≤ (logical cores − 1); priority `BELOW_NORMAL` | Job Object |
| Idle app (not live, buffer off) | ≤ 3 % CPU (NFR-PERF-05) | P1 gate |

## 4. GPU budgets

| Work | Budget | Enforced by |
|---|---|---|
| Engine | ≤ 4 ms per frame (§1); GPU thread priority +7 | Timestamp queries; health warnings |
| NVENC sessions | Reference load = 3 sessions (2 stream + 1 record) | Encoder graph refuses configurations that exceed the queried session limit (OUT-002) |
| **AI and render workers while live** | **0** (CPU paths only) in release one | Governor sets `gpu_allowed=false`; workers construct CPU backends |
| AI and render workers while idle | GPU allowed; GPU thread priority −7 | Worker init |

## 5. Memory budgets

| Process | RAM | VRAM | Enforced by |
|---|---|---|---|
| Engine (excluding replay rings) | ≤ 1.2 GB | ≤ 1.0 GB | Accounting per resource; soak growth ≤ 5 % (NFR-REL-01) |
| Replay rings | bitrate × seconds × 1.1 per canvas (6 Mbps × 60 s ≈ 50 MB) | 0 (CPU-side packets) | Refuses to start if free RAM < requirement + 1 GB (LIB-003) |
| Each browser source | ≤ 300 MB | ≤ 150 MB | CEF process memory reported; Job Object limit 1 GB per browser process |
| App Host + WebView2 | ≤ 600 MB idle (NFR-PERF-05) | — | P1 gate |
| AI worker | ≤ 3 GB (Job Object hard limit) | ≤ 2 GB idle only, after `QueryVideoMemoryInfo` budget check | Job Object; worker preflight |
| Render worker | ≤ 1.5 GB (Job Object hard limit) | ≤ 1 GB idle; 256 MB while live (NVENC only if allowed: release one = MF/CPU while live) | Same |

## 6. Disk and IO budgets

| Work | Budget | Notes |
|---|---|---|
| Recording write | ≤ 60 Mbps sustained | Sequential, unbuffered-aligned writes on the `rec` thread |
| Sidecar event log | ≤ 1 write/s, batched (LIB-002) | |
| Autosave | ≤ 1 write per 2 s per document; never on the UI thread (SYS-001) | Atomic replace |
| Background IO while live | Workers use low IO priority (`FILE_IO_PRIORITY_HINT` / `PROCESS_MODE_BACKGROUND_BEGIN`) | Protects the recording write path |
| Free-space guard | Warn < 10 GB or < 30 min; stop < 1 GB (LIB-007) | Checked every 10 s |

## 7. Startup and responsiveness

| Item | Budget | Ref |
|---|---|---|
| Cold start → interactive UI | ≤ 5 s | NFR-PERF-04 |
| Cold start → first preview frame | ≤ 8 s | NFR-PERF-04 |
| Device probe | ≤ 5 s | ONB-002 |
| UI main-thread task | No task > 16 ms (long tasks are reported in dev builds) | — |
| Library of 1,000 items open | ≤ 1 s | LIB-001 |
| Editor seek (paused, exact frame) | ≤ 150 ms | TL-008 |

## 8. Offline job throughput (idle)

| Job | Budget | Ref |
|---|---|---|
| Transcription | ≥ 4× real time on GPU; ≥ 1× on CPU | SC-11, AI-001 |
| Highlight scoring (Labs) | ≤ 5 min for 4 h after transcription | AI-003 |
| Render 60 s 1080p (NVENC) | ≤ 30 s | EXP-004, SC-08 |
| Validation | ≤ 20 % of render time | EXP-003 |

## 9. Measurement infrastructure
- **Engine stats ring** (shared memory): per-tick GPU timings, lagged/skipped counters, encoder queue depths, output RTT and dropped packets, per-process CPU. Sampled at 1 Hz into the soak log.
- **Soak harness:** scripted reference load against a local RTMP server (MediaMTX container) plus the clapper source. It produces a report with every budget in this file and pass/fail status.
- **CI perf tests:** headless compositor benchmark on a GPU runner (self-hosted, NVIDIA) for §1 and §3; regressions over 10 % fail.
- **Windows Performance Recorder** profiles (`.wprp` checked into the repo) for deep investigations; PIX captures for GPU work.
