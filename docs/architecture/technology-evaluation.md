# Technology Evaluation

Inputs: the Phase 1 requirements (`docs/product/`), ADR 0001 (D1–D5) and the assumptions register (A-xx). Requirements are cited by ID (for example VID-002).

Method: each option is scored against the criteria below on a 1–5 scale (5 = best for Jalakam), using evidence from the requirements and published platform capabilities. A **hard constraint** that rules an option out is called out explicitly, and it overrides any score. Statements about third-party technology are researcher knowledge as of 2026-10 **[BK]** unless cited. Items marked **POC** must be confirmed by the proof-of-concept before implementation depends on them.

## Criteria

| Code | Criterion | What matters for Jalakam |
|---|---|---|
| PERF | Performance | 1080p60 dual canvas, < 0.1 % lagged/skipped frames (NFR-PERF-01), compositor ≤ 2 ms GPU (VID-002) |
| PORT | Portability | Windows-only now (D1); keep a path to macOS/Linux later |
| CAP | Capture APIs | Windows Graphics Capture, Media Foundation, WASAPI, process loopback (A-19) |
| GPU | GPU access | Direct D3D11 device control, shared NT handles, NVENC/AMF/QSV interop, zero-copy |
| AUD | Audio latency | ≤ 20 ms capture-to-mix (AUD-001), MMCSS-scheduled real-time threads |
| PKG | Packaging | Signed installer, size, auto-update (SYS-011) |
| LIC | Licensing | Apache-2.0 product; no GPL-only dependencies (D2, SYS-012) |
| MNT | Maintainability | Long-lived real-time code; refactor safety |
| DBG | Debugging | Crash dumps, symbols, profilers (PIX, Nsight, WPA) |
| SEC | Security | Memory safety, parsing untrusted media/web content, secrets |
| TEAM | Team complexity | Number of languages and toolchains; model-assisted build (Sonnet application layer, Opus media spine) |

---

## E1. Application core: Rust vs C++

| Criterion | Rust | C++ (C++20) |
|---|---|---|
| PERF | 5. Zero-cost abstractions; no GC; same codegen class as C++ (LLVM) | 5 |
| PORT | 4. Cross-platform std; Windows APIs through `windows-rs` (Microsoft-maintained bindings generated from Win32 metadata) | 4 |
| CAP | 4. Every WinRT/Win32/COM API needed (WGC, MF, WASAPI, DXGI) is exposed by `windows-rs`. Vendor samples are C++ and need translating | 5. Vendor samples are native C++ |
| GPU | 4. D3D11/12 and DXGI through `windows-rs`; NVENC through FFmpeg or a bindgen'd SDK header | 5. Direct SDK use |
| AUD | 5. Real-time threads without a GC; MMCSS through Win32 | 5 |
| PKG | 4. Single toolchain (cargo); MSVC ABI | 3. CMake/vcpkg/Conan dependency management is heavier |
| LIC | 5. Ecosystem is mostly MIT/Apache | 4. Mixed; easier to pull in GPL code by accident |
| MNT | 5. Ownership model prevents data races at compile time, which matters for a multi-threaded pipeline running 4 h+ | 3. Concurrency bugs are found at runtime |
| DBG | 4. PDBs, Visual Studio/WinDbg, PIX and WPA work; async stack traces are weaker | 5 |
| SEC | 5. Memory-safe by default; `unsafe` is confined to FFI | 2. Memory-unsafe; the engine parses untrusted data |
| TEAM | 4. One language for core and app host (with Tauri); strong model output quality | 3. C++ core + a different UI language; higher review burden |
| **Total** | **49** | **44** |

**Impractical?** Neither. C++ has better direct access to vendor samples. That is mitigated by using FFmpeg for encoder access and `windows-rs` for OS APIs.
**Choice:** Rust (confirms D3). See ADR-0002.

## E2. Desktop UI: Flutter vs Qt/QML (vs approved Tauri/React)

The brief requires Flutter vs Qt/QML. Tauri 2 + React is included because D3 already approved it and it must be re-checked against the same criteria.

The key requirement is CAN-007 / VID-002: show a 60 fps GPU preview of two canvases without blocking output rendering. In practice the preview must be fed from a GPU texture, not by copying frames through the CPU.

| Criterion | Flutter (Windows) | Qt 6 / QML | Tauri 2 + React (WebView2) |
|---|---|---|---|
| PERF | 4. Impeller/Skia GPU UI; external textures through the texture registrar, which accepts DXGI shared handles on Windows (zero-copy) **POC** | 5. QRhi can import native D3D11 textures into the scene graph; mature for video apps | 3. The UI itself is fast. The preview cannot be drawn inside WebView2 from a GPU texture (no public texture-import API), so it needs a **native child window (airspace)** over the web content |
| PORT | 5 | 5 | 4. WebView2 on Windows, WKWebView on macOS, WebKitGTK on Linux (behaviour differs) |
| CAP | n/a (core) | n/a | n/a |
| GPU | 4. Shared-handle textures | 5 | 3. Preview via native child HWND + swapchain owned by the Rust host; web content cannot overlap that region |
| AUD | n/a | n/a | n/a |
| PKG | 4. ~25–40 MB engine + Rust DLL | 3. Qt DLLs + QML modules + LGPL relink obligations | 4. WebView2 is an OS component (evergreen); small binaries |
| LIC | 5. BSD-3 | 3. LGPLv3 (dynamic linking, relinkable, notices) or commercial. Compatible, but carries compliance work | 5. MIT/Apache; WebView2 runtime is Microsoft-distributed |
| MNT | 4. One UI language (Dart) | 3. QML + JS + C++ glue | 4. TypeScript + React; huge ecosystem (timeline, rich-text transcript editing, virtualised lists) |
| DBG | 4. Flutter DevTools | 3. QML profiler; C++/QML boundary debugging is hard | 5. Chromium DevTools in the shipped runtime |
| SEC | 4 | 4 | 4. Must lock down the WebView (CSP, no remote content, Tauri capability allow-list) |
| TEAM | 3. Adds Dart; Rust bridge via `flutter_rust_bridge` | 2. Adds C++ and QML on top of Rust (cxx-qt bridge), so three languages | 5. TypeScript only, plus Rust; strongest model productivity for a large UI surface (Phase 0 build plan) |
| A11Y (A-14, SYS-008) | 3. UIA support exists; maturity of complex widgets on Windows is lower | 4. Qt Accessibility maps to UIA | 5. Chromium accessibility tree maps to UIA; WCAG tooling (axe) works directly |
| **Total** | **40** | **37** | **42** |

**Impractical?**
- Qt/QML is not impractical. It is the best fit for the preview and is the stack the reference product uses (Phase 0 L01). It loses on team complexity (Rust + C++ + QML) and LGPL compliance overhead.
- Flutter is not impractical, but its Windows accessibility maturity is a risk for the WCAG 2.2 AA target (A-14).
- Tauri has one real limitation, the preview airspace: menus and popovers cannot draw over the preview rectangle. Design rule: no web content overlaps the preview. Selection handles, safe-area guides and other preview overlays are drawn by the native preview renderer.

**Choice:** keep **Tauri 2 + React/TypeScript** (D3) with the preview in a native child window. This is gated by POC-1 part C (frame pacing, resize, DPI, focus and keyboard). **Fallback if POC-1C fails: Flutter** (shared-handle texture, all-Rust bridge, BSD licence). Qt/QML is rejected as primary. See ADR-0003.

## E3. Media engine: established integration vs custom engine

| Criterion | libobs (OBS core) | GStreamer 1.24+ (d3d11, nvcodec, wasapi2 plugins) | Custom real-time pipeline + FFmpeg libraries |
|---|---|---|---|
| PERF | 5. Proven | 4. Capable; dynamic pipeline reconfiguration (outputs added/removed, encoder sharing) adds latency and complexity | 5. Designed exactly for one frame clock, two canvases, encoder sharing and a packet ring buffer **POC** |
| PORT | 5 | 5 | 3. Windows-specific capture/compositor modules; FFmpeg is portable |
| CAP | 5. WGC, game capture, WASAPI | 4. WGC (d3d11screencapturesrc), wasapi2src, MF video | 4. Written against WGC, MF and WASAPI directly via `windows-rs` |
| GPU | 5 | 4 | 5. Full control over the D3D11 device, shared handles and NVENC hwframes |
| AUD | 4 | 4 | 4. Our own WASAPI + mixer with MMCSS |
| PKG | 4 | 2. Large runtime (100+ MB of plugins); plugin registry cache | 4. FFmpeg DLLs (~40–60 MB LGPL build) |
| LIC | **1. GPL-2.0+. Hard constraint: violates D2.** | 3. LGPL core, but plugin licences vary (`-ugly`, x264enc are GPL); needs careful curation | 5. LGPL FFmpeg build (no `--enable-gpl`, no nonfree) + MIT/Apache crates |
| MNT | 3. Large C codebase; would be a fork | 3. Pipeline graphs are hard to reason about under failure | 4. Our code; scoped to what we need |
| DBG | 3 | 2. Debugging across many elements, threads and caps negotiation | 4 |
| SEC | 3 | 3 | 4. Media parsing (FFmpeg) isolated in workers where possible (see the threat model) |
| TEAM | 2. C + plugin model | 3. Rust bindings (gstreamer-rs) are excellent | 3. Most code to write; this is the Opus-owned media spine |
| **Total** | n/a (excluded) | **37** | **45** |

**Impractical?** libobs is **excluded** (GPL vs D2). GStreamer is viable. It is rejected because Jalakam's core needs (one master clock across two canvases, encoder sharing with live add/remove, a per-consumer drop policy, and a packet ring buffer with keyframe-aligned saves) are easier to make deterministic and testable in a purpose-built pipeline. GStreamer also adds packaging weight and plugin-licence curation.

**Choice:** a custom real-time pipeline (capture, clock, compositor, encoder graph, outputs) written in Rust on Windows APIs. FFmpeg libraries (LGPL) are used for encoder access (`h264_nvenc` with D3D11 hardware frames, `h264_mf`, `h264_amf`, `h264_qsv`), muxing (FLV/RTMP, MKV, MP4), decoding and export rendering. See ADR-0004.

## E4. AI and rendering: in-process modules vs isolated worker processes

| Criterion | In-process (inside the engine or app host) | Isolated worker processes |
|---|---|---|
| PERF | 4. No IPC overhead | 4. IPC is control-only; media is passed by file path, so overhead is negligible |
| Resource bounding | 1. Cannot cap the memory or CPU of a module inside a shared process | 5. Windows Job Objects cap memory (`JOB_OBJECT_LIMIT_PROCESS_MEMORY`) and CPU rate (`JOB_OBJECT_CPU_RATE_CONTROL`) and set priority class; a job can be killed instantly |
| Fault isolation | 1. A crash in whisper.cpp, ONNX Runtime or FFmpeg decode of a malformed file kills the host | 5. A worker crash fails only that job (AI-007, NFR-REL-03) |
| GPU | 3 | 4. Lower process GPU priority; separate D3D/CUDA contexts |
| SEC | 2. Untrusted media parsed in a privileged process | 4. Workers can run at reduced privilege (restricted token / low integrity) |
| DBG | 4 | 3. Multi-process debugging; mitigated by correlation IDs in logs |
| TEAM | 4 | 3 |

**Hard constraint:** AI-007 and NFR-REL-03 (AI must never harm the live output; a single failure must not kill outputs) make in-process AI impractical inside the engine.
**Choice:** isolated workers (`jalakam-worker --role=ai|render`) inside Job Objects. See ADR-0005 and ADR-0011.

## E5. Local-first AI vs optional cloud inference

| Criterion | Local-first (whisper.cpp, ONNX Runtime) | Cloud inference (ASR/vision APIs) |
|---|---|---|
| Privacy | 5. Media never leaves the PC (D4, SYS-007) | 2. Uploads the user's media |
| Cost | 5. No per-minute cost to Jalakam or the user | 2. Per-minute ASR fees; who pays is unresolved |
| Latency | 4. ≥ 4× real-time on the reference GPU (SC-11) | 3. Upload time for hour-long recordings dominates |
| Quality | 4. Whisper-class accuracy is adequate (SC-12) | 5 |
| Resource use | 2. Competes with the stream, so it needs the governor (ADR-0011) | 5 |
| Offline | 5 | 1 |

**Choice:** local-first for ASR, VAD, scoring and (Labs) detection. Cloud is used only for LLM text drafting (AI-006), opt-in with the user's own key, behind a provider adapter. A cloud ASR adapter is kept as an interface but not implemented in release one. See ADR-0010.

## E6. Single-process vs controlled multi-process

| Criterion | Single process | Controlled multi-process (app host, engine, workers, browser, crash handler) |
|---|---|---|
| PERF | 5 | 4. The preview crosses a process boundary via a shared GPU texture (one GPU copy, < 0.2 ms) |
| Fault isolation | 1. A UI-layer panic, a browser-source crash or an AI crash ends the stream | 5. **The engine keeps streaming and recording if the UI host crashes** and reattaches when it restarts |
| Resource bounding | 1 | 5. Per-process Job Objects and priorities |
| SEC | 2 | 4. Untrusted web content (browser sources) and media parsing are isolated |
| DBG | 5 | 3 |
| PKG | 5 | 4. Several executables in one installer |
| TEAM | 4 | 3. IPC contracts must be versioned |

**Hard constraint:** NFR-REL-03 (fault isolation) and SCN-003 (a browser-source crash must not affect outputs) make a single process impractical. CEF is multi-process by design anyway.
**Choice:** controlled multi-process with a small, fixed set of process roles and versioned IPC. See ADR-0005.

## E7. Supporting choices (summary; details in ADRs)

| Topic | Chosen | Rejected (reason) | ADR |
|---|---|---|---|
| GPU API for the compositor | Direct3D 11 | D3D12 (much more code for the MVP, with no measured need; D3D11 interop with WGC, MF and FFmpeg hwframes is mature); wgpu (abstraction hides the shared-handle and keyed-mutex control we need) | ADR-0006 |
| Screen/window capture | Windows Graphics Capture | DXGI Desktop Duplication (display only; kept as fallback); hook-based game capture (out of scope, anti-cheat risk) | ADR-0007 |
| Camera | Media Foundation source reader with a D3D11 device manager | DirectShow (legacy; kept for virtual-camera inputs if needed) | ADR-0007 |
| Audio | WASAPI event-driven + MMCSS "Pro Audio"; process loopback where supported | ASIO (licensing, niche), WDM-KS | ADR-0007 |
| Browser source | CEF off-screen rendering with shared D3D11 texture | WebView2 (no off-screen texture output) | ADR-0007 |
| IPC | Named pipes (current-user ACL) carrying versioned JSON-RPC for control; shared memory and DXGI NT handles for data | gRPC/HTTP on localhost (opens a port; NFR-SEC-02); COM out-of-process servers (complexity) | ADR-0008 |
| Scene state | App host is the source of truth; the engine reconciles a declarative desired state | Engine-owned state (state lost on engine crash; harder undo) | ADR-0009 |
| Persistence | SQLite (WAL) for indexes and job/publish state; versioned JSON documents with atomic replace for scene collections and projects | All-SQLite (loses human-diffable projects, A-17); all-JSON (no transactional job state) | ADR-0012 |
| Logging and crashes | `tracing` (JSON lines) + Crashpad handler process; opt-in upload | In-process crash reporting (unreliable on stack corruption) | ADR-0015 |

## E8. Requirements that make candidates impractical (summary)

| Requirement | Makes impractical |
|---|---|
| D2 / SYS-012 (no GPL-only) | libobs, x264, GPL-only GStreamer plugins, FFmpeg builds with `--enable-gpl` or `--enable-nonfree` |
| NFR-REL-03, SCN-003, AI-007 | Single-process design; AI or rendering inside the engine process |
| VID-002 (GPU-resident path) + CAN-007 | Rendering the preview inside WebView2 via CPU frame copies (~500 MB/s at 1080p60 × 2 canvases) |
| NFR-SEC-02 (no listening sockets by default) | Localhost HTTP/gRPC IPC |
| A-14 (WCAG 2.2 AA) | Not disqualifying, but lowers Flutter's suitability |
| D1 (Windows-only MVP) | No candidate. Portability is a tie-breaker only |
