# Assumptions and Decisions Needed

Phase 1 input: the Phase 0 research pack (`docs/research/`) and ADR 0001 (`docs/decisions/0001-phase0-decisions.md`).

Rule: a decision is never invented. When the inputs do not settle a question, it is recorded here with a **proposed default** and a rationale. Requirements that depend on an item carry its ID in their Dependencies column. A default stays in force until the owner confirms or replaces it.

Status values: **Approved** (decided in ADR 0001), **Default** (proposed, working assumption, needs confirmation), **Open** (no safe default; must be answered by the date given).

## 1. Approved decisions (inputs, not re-opened)

| ID | Decision | Source |
|---|---|---|
| D1 | Windows-only MVP: Windows 10 2004+ / 11, x64, NVIDIA-first, Media Foundation software fallback | ADR 0001 |
| D2 | Apache-2.0. FFmpeg as a dynamically linked LGPL build; NVENC/AMF/QSV; no x264, no libobs, no GPL-only dependencies | ADR 0001 |
| D3 | Rust core + Tauri/React (TypeScript) UI; the core renders the preview into a native child surface | ADR 0001 |
| D4 | Local ASR and scoring; no media leaves the PC by default; cloud LLM for metadata only, opt-in, user's own key | ADR 0001 |
| D5 | Local multistream to at most 3 outputs, no cloud relay; YouTube is the only publishing target | ADR 0001 |

## 2. Assumptions with proposed defaults

| ID | Question | Proposed default | Rationale | Affects | Status |
|---|---|---|---|---|---|
| A-01 | Reference hardware for performance targets | Windows 11 23H2+, NVIDIA RTX 3060 12 GB, 6-core CPU (Ryzen 5 5600 / i5-12400 class), 16 GB RAM, NVMe SSD, 1080p60 single monitor | Mid-range streaming PC; NVIDIA-first per D1 | All NFR-PERF | Default |
| A-02 | Minimum supported hardware | NVIDIA GTX 16-series / RTX 20-series or newer with NVENC; 4-core CPU; 8 GB RAM. Older GPUs run the software encoder with reduced targets | Turing-or-newer NVENC quality is adequate for streaming; keeps the test matrix small | ONB-002, VID-003 | Default |
| A-03 | Maximum canvas resolution and frame rates in release one | Landscape up to 1920×1080; portrait fixed at 1080×1920; 30 and 60 fps (also 24/25/50 selectable); 1440p/4K deferred to P1 | Matches the Phase 0 MVP thesis; limits GPU budget for the dual canvas | VID-001, CAN-001/002 | Default |
| A-04 | Live transcription in release one | **Moved from MVP to experimental (P2).** Release one transcribes after recording only | Phase 0 placed live ASR in the MVP, but risk R02 (ASR stealing GPU/CPU from the stream) threatens the stability thesis. Post-recording ASR proves the editing value with zero live risk | AI-001, AI-004 | **Default; needs explicit confirmation because it changes the Phase 0 MVP** |
| A-05 | Release-one placement of YouTube publishing | Built in release one but switched on only after the export reliability gate (G4) passes; otherwise it ships in 1.1 | D5 puts YouTube in the MVP; the Phase 1 brief prefers one validated integration *after* local export is reliable. A gated feature satisfies both | EXP-005/006 | Default |
| A-06 | ASR engine and model | whisper.cpp (MIT) with a multilingual "small" or "base" model downloaded on first use; English UI, transcription language auto-detected | Permissive licence (D2), local (D4), CPU and GPU builds, mature | AI-001, AI-008 | Default |
| A-07 | Face/person detection for reframing (experimental) | ONNX Runtime (MIT) with a permissively licensed detector; model licence checked before adoption | D2 licence constraint | AI-005 | Open: choose model by M3 |
| A-08 | Default cloud LLM for metadata drafting | Anthropic Claude Haiku 5.5 (`claude-haiku-5-5`), user's own API key; provider abstraction kept | D4; cheapest capable text model; the key is the user's, so no Jalakam running cost | AI-006 | Default |
| A-09 | Twitch chat ingest for highlight scoring | Read-only anonymous IRC/EventSub chat connection to the user's own channel; no posting | Feeds the chat-velocity signal (Phase 0 F21) without needing write scopes | AI-003 | Default |
| A-10 | Recording container | MKV during capture (crash-safe), automatic lossless remux to MP4 on stop; user can choose fragmented MP4 instead | Crash safety (Phase 0 U05, M5 gate) plus compatibility | VID-006, SYS-002 | Default |
| A-11 | Export baseline format | MP4 (faststart), H.264 High profile, yuv420p, AAC-LC 48 kHz stereo, constant frame rate equal to the source | Accepted by every major platform; hardware encode available | EXP-002/003 | Default |
| A-12 | Platform length and size limits used in export validation | Stored as data in versioned preset files, not hard-coded; values re-checked against official platform docs at implementation time and dated in the preset | Platform limits change often; this avoids baking stale numbers into code | EXP-003 | Default |
| A-13 | Telemetry | Off by default; opt-in prompt during onboarding; crash dumps stored locally and uploaded only with consent | D4 privacy stance | SYS-003, SYS-007 | Default |
| A-14 | Accessibility target | WCAG 2.2 AA for all non-preview UI by release one; keyboard-complete operation of the live controls | Tauri/WebView2 exposes UI Automation; AA is the common industry baseline | SYS-008 | Default |
| A-15 | Localisation | English only in release one; all strings externalised from day one | Keeps scope small without blocking later localisation | All UI | Default |
| A-16 | Installer and updates | Signed NSIS or MSI installer through the Tauri bundler; auto-update with signed manifests; config snapshot before each upgrade | Matches the Phase 0 L07 pattern; signing avoids SmartScreen friction | SYS-011, SYS-010 | Open: a code-signing certificate must be bought (cost and identity) by M5 |
| A-17 | Scene collection and project file format | Versioned JSON with a schema version field and forward migrations; IDs are UUIDv7 | Human-diffable; supports migration snapshots | SCN-001, TL-003 | Default |
| A-18 | AMD AMF and Intel QSV encoders | P1: in release one if M2 is on schedule, otherwise 1.1 | D2 permits them; D1 says NVIDIA-first | VID-003 | Default |
| A-19 | Per-application audio capture | P1; available only on Windows builds that support the process-loopback API (Windows 11 and recent Windows 10 server builds); hidden elsewhere | API availability is OS-build-dependent; desktop loopback covers the P0 need | AUD-003 | Default |
| A-20 | Local automation API in release one | P1; localhost only, token-authenticated, JSON-RPC over WebSocket | Phase 0 F27 was a thin MVP item; security review needed (R10) | SYS-009 | Default |
| A-21 | Highlight-suggestion acceptance target | precision@10 ≥ 0.4 on an internal eval set of ≥ 20 h of the team's own VODs | Carried from the Phase 0 M3 gate; there is no external benchmark | AI-003 | Default |
| A-22 | Storage locations | Recordings: `%USERPROFILE%\Videos\Jalakam`; app data: `%APPDATA%\Jalakam`; models and cache: `%LOCALAPPDATA%\Jalakam` | Windows conventions; keeps large files out of roaming profiles | LIB-001, AI-008 | Default |
| A-23 | Product name and branding | "Jalakam" is the working name; trademark search not yet done | Phase 0 R08 | All | Open: trademark check before public beta |
| A-24 | YouTube API compliance | Apply for OAuth app verification and quota as soon as the app is registered; unverified mode (test users only) until approved | Publishing cannot reach users without verification (Phase 0 U08) | EXP-005 | Open: start by M2 |

## 3. Decisions needed (summary for the owner)

These Open items and the one Default that changes earlier scope need an answer before the stated milestone.

| # | Decision | Needed by | Proposed answer |
|---|---|---|---|
| 1 | Confirm A-04: move live transcription out of release one | Before M0 architecture | Yes, move it |
| 2 | Confirm A-05: YouTube publishing gated behind the export reliability gate | Before M0 | Yes |
| 3 | A-16: buy a code-signing certificate (OV or EV) | M5 | OV certificate; EV if the budget allows |
| 4 | A-07: choose the reframing detection model | M3 | Shortlist two permissive ONNX models and benchmark them |
| 5 | A-23: run a trademark check for "Jalakam" | Before public beta | Run the check |
| 6 | A-24: register the Google Cloud project and start OAuth verification | M2 | Start now |
