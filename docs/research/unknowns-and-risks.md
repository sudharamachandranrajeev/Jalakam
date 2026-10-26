# Unknowns and Risks

Labels: **[O]** observation, **[H]** hypothesis, **[R]** recommendation.

## 1. Evidence tiers

| Tier | Meaning | Examples in this pass |
|---|---|---|
| A. Public documentation | Vendor docs, release notes, public API spec | S01–S16 |
| B. Normal application use | Manual UI walkthrough, screenshots, settings, the user's own exported config | Only config shape (L06) and archive names (L07) were examined. **The UI walkthrough was not done.** |
| C. Static packaging inspection | File inventory, manifest, PE resources, printable-string presence | L01–L05 |
| D. Authorised deeper investigation | Disassembly, runtime tracing, network capture | **Not performed.** See §4 |

## 2. Static inspection findings (Tier C, functional level only)

[O] These come from the installed package version 0.10.7.7. A string or file being present shows that a capability is compiled in. It does not show how the capability works or whether it is enabled.

- UI: Qt 6 Quick/QML with Qt WebChannel and WebSockets. Browser sources use CEF (`libcef.dll`, `Browser Shell.exe`). A WebView2 runtime is bundled. (L01)
- Media: FFmpeg; encoder paths for NVENC (D3D11 + D3D12), x264, AMD AMF and Media Foundation; Media Foundation capture; Spout2; a VST3 host. (L01, L04)
- Platform: the Discord Game SDK (rich presence), a virtual camera (`virtual_camera.dll` plus installer), Sentry and Crashpad crash reporting, and a metrics-ingest endpoint. (L01, L04)
- Twitch: client-config fetch from Twitch's contribute ingest service for Enhanced Broadcasting. (L04, consistent with S17)
- Platform identifiers present: Twitch, YouTube, TikTok, Kick, Fourthwall, X share intent. (L04)
- Speech-to-text and caption track strings are present. (L04) No public doc was found. **Unknown:** whether this ships to users, is local or cloud, and which model it uses.
- No SRT, WHIP or WebRTC output strings were found. (L04, bounded search)
- REA PE-resource inspection shows only icons, version and manifest resources (L02). REA found no Rust auditable metadata (L03).

## 3. Unknowns

| ID | Unknown | Why it matters | How to resolve (least invasive first) |
|---|---|---|---|
| U01 | Real CPU/GPU cost of Meld's always-on clip encoder | Sets the budget for our ring buffer plus ASR | Tier B: Task Manager / GPU-Z during normal use, with clipping on and off |
| U02 | End-to-end A/V latency and sync handling | Sync is the top stability risk | Tier B: clapper test recorded locally |
| U03 | First-run flow details (permissions, sign-in requirement) | Onboarding design | Tier B: fresh Windows user profile, screen recording |
| U04 | Whether STT/captions is live, local or cloud | Competitive overlap with our AI spine | Tier A: release notes or Discord; Tier B: look for the setting in the UI |
| U05 | Clip container behaviour on crash (fMP4 vs moov-at-end) | Data safety | Tier B: kill the process during recording and inspect the file with ffprobe |
| U06 | Encoder-sharing rules across canvases | Multistream encoder graph | Tier A: S03 plus Tier B: Encoder View screenshots |
| U07 | How the Portrait canvas is composited (second render pass vs crop) | Dual-canvas GPU cost | Tier B: GPU timing with Multi Canvas on and off. Do **not** infer from binaries |
| U08 | Platform API approval timelines (TikTok Content Posting, YouTube upload quota) | Publishing feasibility | Tier A: apply early; it is a business process |
| U09 | Twitch EB eligibility for third-party apps | Whether F13 can be built at all | Tier A: Twitch developer contact; S17 is the IVS equivalent |
| U10 | Highlight-scoring accuracy without labelled data | AI value proof | Build our own eval set from the user's own VODs |

## 4. When deeper investigation would be justified
[R] None of the unknowns above needs Tier D. Each can be answered by docs, normal use, or measuring our own build. Consider Tier D only if all of these hold:
1. A specific question (for example U07) blocks a design decision.
2. Tiers A–B have failed.
3. The user confirms that the licence/EULA and local law permit it.
4. The analysis looks at behaviour (timings, API calls made) and does not copy implementation.

Available tooling: REA MCP (static PE/resource/strings, plus runtime observation), Python 3.14, git, and GitHub CLI. Ghidra, IDA and debugger availability was not checked, because no question required them.

## 5. Risks

| ID | Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|---|
| R01 | GPU compositor + capture + encode pipeline drops frames or drifts A/V sync | High | Critical | Build the spine first; a soak test (4 h stream, record and buffer together) is the MVP exit gate; zero-copy GPU texture path |
| R02 | Local ASR steals GPU/CPU from the game and encoder | High | High | Run ASR on CPU at low priority or on a small GPU model; degrade gracefully; option to transcribe post-stream |
| R03 | AI highlight quality is too low to be useful | Medium | High | Hybrid signals (audio peaks, chat velocity, events); human review queue; measure precision@10 on an eval set |
| R04 | Licence contamination (GPL from libobs/x264, LGPL FFmpeg, VST3 SDK, NVENC SDK terms) | Medium | High | Decide the licence before writing code (decision D2); keep a third-party licence register |
| R05 | Clean-room breach (implementers see the reference's code or internals) | Low | High | Implementers get only this pack and public specs; no decompilation; identifiers are not reused |
| R06 | Platform API access refused or slow (TikTok, Twitch EB) | Medium | Medium | MVP publishes to YouTube only; other platforms are export-only |
| R07 | Anti-cheat flags injected game capture | Medium | Medium | Use Windows Graphics Capture only in the MVP |
| R08 | Trademark/trade-dress similarity to Meld | Low | Medium | Independent name (Jalakam), UI and icons |
| R09 | Scope creep toward full Meld parity before the AI value is proven | High | High | MVP scope lock (see `mvp-recommendation.md`) |
| R10 | Stream-key/OAuth token leakage | Medium | High | OS credential vault (Windows DPAPI/Credential Manager); never log keys; redact in crash reports |
