# Component Boundaries

This document defines each component's process, the data it owns, its interface and events, its dependencies, and what it must not do. Interfaces are sketched as Rust traits (in-process) or IPC methods (cross-process, ADR-0008). They are contracts, not final signatures.

**Build ownership** follows the Phase 0 plan: **Opus** (Opus 5.5, high effort) owns the media spine and reviews every security-relevant change. **Sonnet** (Sonnet 5.5, high effort) owns the application layer.

Crate layout (Cargo workspace; the names are the boundaries):

```
crates/
  jk-protocol      # IPC message types + versioning (shared by all processes)
  jk-model         # scene/project/EDL document types + schema migrations (pure, no IO)
  jk-app           # App Host core (state store, orchestration, governor, supervisor)
  jk-persist       # SQLite + atomic document store + migrations
  jk-library       # media index, probe, content hashing, derived-data cache
  jk-publish       # publishing adapter trait + YouTube adapter
  jk-secrets       # Credential Manager / DPAPI wrapper
  jk-observe       # tracing setup, log rotation, diagnostics bundle, crash client
  jk-engine        # engine process main + reconciler
  jk-capture       # WGC, MF camera, WASAPI, process loopback, CEF bridge
  jk-compositor    # D3D11 render graph, effects, colour conversion, preview producer
  jk-encode        # encoder graph, FFmpeg encoder wrappers, AAC
  jk-output        # RTMP outputs, recorder, replay ring, muxers
  jk-audio         # mixer, resampler, filters, meters
  jk-worker        # worker process main (ai | render roles)
  jk-ai            # ASR, VAD, scoring, (Labs) detection, LLM adapter
  jk-render        # EDL → FFmpeg graph, render manifest, validator
apps/
  jalakam          # Tauri app (Rust host + React UI in ui/)
  jalakam-engine
  jalakam-worker
  jalakam-browser  # CEF subprocess executable
  jalakam-crash
```

Dependency rule: `jk-model` and `jk-protocol` depend on nothing internal. Engine crates never depend on `jk-app`, `jk-persist`, `jk-publish` or `jk-secrets`. UI code never talks to the engine directly. CI enforces this with `cargo deny` plus a dependency-graph check.

---

## 1. UI and application state

| | |
|---|---|
| **Process / owner** | App Host. React UI in WebView2 + `jk-app` state store. Sonnet |
| **Owns** | UI-only state (panels, selections, focus); the **application state store**: an authoritative, revisioned snapshot of collections, scenes, outputs, library view, jobs and health |
| **Interface (UI → host)** | Tauri commands: `cmd(name, args) -> Result<T, AppError>`. Every mutating command carries `trace_id` and returns the new store revision |
| **Events (host → UI)** | `state.delta {rev, patches[]}` (JSON Patch), `state.snapshot`, `toast`, `meters` (30 Hz), `stats` (1 Hz) |
| **Depends on** | Scene/project manager, library, job orchestrator, publishing, governor (all in-process) |
| **Must not** | Hold media; call the engine directly; block on IO in a command handler (long work becomes a job or an async task); render web content over the preview rectangle (ADR-0003) |
| **Failure** | A WebView renderer crash: the host reloads the UI and resends the snapshot. A host crash: the watchdog relaunches it and the engine is reattached (system-design §5.1) |

## 2. Scene / project management

| | |
|---|---|
| **Process / owner** | App Host, `jk-model` + `jk-app::scenes`. Sonnet (schemas reviewed by Opus) |
| **Owns** | Scene collections, scenes, per-canvas layer stacks, transforms, transitions, source definitions (the **desired state**); clip projects (EDL + variants); undo/redo stacks |
| **Interface** | `trait SceneStore { fn apply(&mut self, cmd: SceneCmd) -> Result<Rev>; fn undo(); fn redo(); fn snapshot(&self) -> SceneDoc; fn export(path); fn import(path) -> ImportReport }` and the equivalent `ProjectStore` for clip projects |
| **Outbound** | On each revision: `engine.applyScene(rev, patch)` (only the parts the engine needs); persistence autosave |
| **Depends on** | Persistence (documents), library (media references) |
| **Must not** | Contain device handles or GPU objects; mutate a document without an undo entry; write files directly (persistence owns IO) |
| **Failure** | An invalid command is rejected with `SCN.*` codes and the state is unchanged. Engine reconcile failure for a source is reported per source and the desired state is kept |

## 3. Video and audio capture

| | |
|---|---|
| **Process / owner** | Engine, `jk-capture`. **Opus** |
| **Owns** | Device sessions: WGC capture items and frame pools, MF source readers, WASAPI clients, process-loopback activations, CEF browser hosts; the device inventory (probe) |
| **Interface** | `trait VideoSource { fn start(&mut self, dev: &D3D11Device) -> Result<()>; fn latest(&self) -> Option<FrameRef>; fn status(&self) -> SourceStatus; fn stop(&mut self) }` · `trait AudioSource { fn start(...); fn ring(&self) -> &SpscRing<AudioBlock>; ... }` · `FrameRef` = D3D11 texture + QPC timestamp + colour info |
| **Events** | `source.status {id, state, reason}`, `device.added/removed` |
| **Depends on** | The D3D11 device (from the compositor), OS APIs |
| **Must not** | Block the render thread; copy frames to the CPU on the NVENC path; retry device opens in a tight loop (backoff ≥ 1 s) |
| **Failure** | Device loss → `Degraded` with a placeholder frame or silence → reattach on `device.added` with the same hardware ID (ONB-003, AUD-009). CEF crash → restart up to 3 times per minute |

## 4. Compositing and preview

| | |
|---|---|
| **Process / owner** | Engine, `jk-compositor`. **Opus** |
| **Owns** | The D3D11 device and immediate context (single owner: render thread); canvas render targets; effect shaders; colour-conversion passes; preview texture ring (shared NT handles with keyed mutex) |
| **Interface** | `fn reconcile(scene: &EngineScene)` (applied at the next tick); `fn render_tick(t: Qpc) -> CanvasFrames`; `fn preview_handles() -> PreviewRing` (handles duplicated into the App Host on attach) |
| **Events** | `render.stats {gpu_ms, lagged}` (into the shared-memory stats ring) |
| **Depends on** | Capture (frame mailboxes), clock |
| **Must not** | Wait on encoders or preview consumers (preview drops frames when the presenter is slow); allocate GPU memory per frame after warm-up |
| **Failure** | Device removed (TDR) → recreate the device, rebuild resources, restart sources; outputs keep their sockets and receive repeated-frame filler up to 2 s, then fail if recovery has not finished. **POC** |

## 5. Encoding, outputs and recording

| | |
|---|---|
| **Process / owner** | Engine, `jk-encode`, `jk-output`, `jk-audio` (AAC). **Opus** |
| **Owns** | The encoder graph (sessions keyed by settings); packet broadcast; outputs (RTMP clients); recorder (MKV writer, remux); replay rings |
| **IPC interface** | `outputs.configure(specs)`, `outputs.start(ids, deadline)`, `outputs.stop(ids)`, `recording.start/stop`, `replay.configure/start/stop`, `replay.save(canvas, seconds) -> path`, `marker.add(label)` |
| **Events** | `output.state {id, state, reason, attempt}`, `recording.state`, `replay.saved {path, duration}`, `encoder.graph {sessions[]}` |
| **Depends on** | Compositor (canvas frames), audio mixer, OS sockets/files. Secrets are passed **in** by the App Host per start; the engine never reads the vault |
| **Must not** | Back-pressure encoders from a slow consumer; log stream keys or URLs containing keys (redaction at the type level: a `Secret<String>` without `Debug`/`Display`); write outside the configured media folders |
| **Failure** | ADR-0014: output-local failure; an encoder failure stops only its subscribers and offers the fallback; disk-full stops the recorder cleanly |

## 6. Media library and timeline

| | |
|---|---|
| **Process / owner** | App Host, `jk-library` + timeline model in `jk-model`; editor UI in React. Sonnet |
| **Owns** | Library index (SQLite): media items, session records, markers, transcripts index, derived-data cache entries keyed by content hash (BLAKE3 of file chunks + size) |
| **Interface** | `trait Library { fn register(path, origin) -> ItemId; fn get(id); fn query(filter) -> Page<Item>; fn relink(id, path); fn derived(id, kind) -> Option<Path> }` · Timeline: `ProjectStore` (see §2) + `fn frame_at(project, variant, t) -> FrameRequest` (served by the render worker for exact paused frames, ADR-0016) |
| **Events** | `library.changed {ids}` |
| **Depends on** | Persistence, render worker (probe, thumbnails, exact frames), AI orchestration (transcripts) |
| **Must not** | Open source media for writing; parse media in the App Host (probing runs in a worker, because FFmpeg parsing of untrusted files is isolated) |
| **Failure** | A missing file is marked `missing` with relink; a corrupt index is rebuilt from disk scan + sidecars |

## 7. AI inference and orchestration

| | |
|---|---|
| **Process / owner** | Orchestration: App Host `jk-app::jobs` + governor. Inference: worker `--role=ai`, `jk-ai`. Sonnet (governor reviewed by Opus) |
| **Owns** | Job definitions, model registry (versions, checksums, paths), AI outputs (transcripts, scores, suggestions) |
| **Worker protocol** | `job.start {id, kind, inputs[ro paths], params, budget}` → `job.progress {pct, checkpoint}` → `job.result {outputs[], metrics}` / `job.error`; control: `job.pause`, `job.resume`, `job.cancel`, `budget.update {cpu_rate, gpu_allowed, mem_limit}` |
| **Adapters** | `trait AsrEngine` (whisper.cpp impl), `trait Detector` (ONNX, Labs), `trait LlmProvider` (Anthropic impl, opt-in, BYO key; sends only transcript text and user context) |
| **Depends on** | Library (inputs), governor (budget), secrets (LLM key), persistence (job state) |
| **Must not** | Run inside the engine; use the GPU while any output is live (release-one policy, ADR-0011); send media to any network service |
| **Failure** | Worker crash → job `Failed(retryable)` resuming from the last checkpoint; model checksum mismatch → re-download (AI-008) |

## 8. Render queue

| | |
|---|---|
| **Process / owner** | Queue: App Host `jk-app::jobs`. Execution: worker `--role=render`, `jk-render`. Sonnet (determinism tests reviewed by Opus) |
| **Owns** | Export jobs, presets (versioned data files), render manifests, validation reports |
| **Interface** | `render.enqueue(project_id, variant_ids[], preset_id) -> JobId[]`, `render.reorder`, plus the same job control as §7. Worker: `job.start {kind: Render, manifest}` |
| **Depends on** | Library, project store, presets, governor |
| **Must not** | Render from an unsaved project revision (the manifest pins the revision); overwrite an existing export (new file name per job) |
| **Failure** | Hardware encode failure retries once in software (EXP-004); validation failure marks the output invalid (EXP-003) |

## 9. Publishing adapters

| | |
|---|---|
| **Process / owner** | App Host, `jk-publish`. Sonnet, with mandatory Opus security review |
| **Owns** | Publish records (SQLite): target, file hash, metadata, upload session URI, state |
| **Interface** | `trait PublishAdapter { fn id(&self) -> &str; fn validate(&self, file: &ValidatedExport, meta: &Meta) -> Vec<Issue>; async fn begin(&self, ...) -> Session; async fn resume(&self, s: &Session) -> Progress; async fn status(&self, s) -> PublishState; fn analytics(&self, ...) (P2) }` · Implementations: `YouTubeAdapter` (release one, gated by A-05) |
| **Depends on** | Secrets (OAuth tokens), validator (only `ValidatedExport` values can be published; the type is constructed only by the validator) |
| **Must not** | Publish without an explicit user confirmation event carrying the record ID; retry a rejected upload automatically; store tokens outside the vault |
| **Failure** | Network loss → resume from the last confirmed byte; auth error → `ReconnectRequired`; quota → `Failed(quota)` with the API message |

## 10. Persistence and migrations

| | |
|---|---|
| **Process / owner** | App Host, `jk-persist`. Sonnet (reviewed by Opus) |
| **Owns** | `%APPDATA%\Jalakam\jalakam.db` (SQLite WAL): library, jobs, publish records, settings index; documents: `collections/*.jkc.json`, `projects/*.jkp.json` with `.history/` (last 10); `prefs.json`; `archives/` migration snapshots |
| **Interface** | `trait DocStore { fn load<T: Versioned>(path) -> Result<T>; fn save_atomic<T>(path, &T) -> Result<()> }` · `Db::tx(|tx| ...)` · `fn migrate_all() -> MigrationReport` (snapshot first) |
| **Must not** | Be called from engine or worker processes (they receive data over IPC and write only media/derived files they own) |
| **Failure** | Atomic replace failure keeps the previous version; migration failure restores the snapshot and opens read-only (SYS-010) |

## 11. Logging, telemetry and crash recovery

| | |
|---|---|
| **Process / owner** | All processes use `jk-observe`; `jalakam-crash.exe` handler. Sonnet (redaction reviewed by Opus) |
| **Owns** | Logs `%LOCALAPPDATA%\Jalakam\logs\<process>-<date>.jsonl` (rotating 10 × 10 MB per process); crash dumps; opt-in telemetry queue |
| **Interface** | `jk_observe::init(process_role)`; spans carry `trace_id` across IPC; `diagnostics.export() -> zip` (redacted); `telemetry.record(event)` (no-op unless opted in) |
| **Crash recovery** | On start the App Host checks the "dirty session" marker. If set, it offers restore: re-register and repair recordings, resume the job queue, reload the last collection revision (SYS-002) |
| **Must not** | Log secrets, stream URLs with keys, OAuth codes or chat text; include full-memory dumps (minidumps with stacks + modules only, no heap) |

---

## Cross-cutting interface rules

1. **Versioning:** every IPC message has `v` (protocol major.minor). Peers negotiate on connect; a major mismatch refuses the connection and triggers a restart of the out-of-date child.
2. **Idempotency:** mutating engine RPCs carry a request ID. Repeats are no-ops (needed for App Host reattach).
3. **Deadlines:** every RPC has a deadline (default 2 s; start operations 10 s).
4. **Paths:** cross-process paths are absolute, canonicalised, and validated against allowed roots (media folders, cache, temp) by the receiver.
5. **Secrets** cross IPC only App Host → engine (stream keys at start) and never in events or logs.
