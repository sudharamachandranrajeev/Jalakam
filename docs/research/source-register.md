# Source Register

Research pass date: **2026-10-26**. Researcher model: Claude Opus 5.5 (medium effort).

Every factual claim in this research pack cites one of the IDs below. Vendor-authored sources are flagged because they describe the vendor's own product and are not independently verified.

## Clean-room note

This pack is a functional specification input for an independent implementation (working name **Jalakam**). It contains no Meld Studio source code, no decompiled code, no copied QML/JavaScript, no credentials and no user configuration contents. Binary-level findings are recorded only as functional or third-party-component facts (for example "ships an NVENC encoder path"), not as internal designs to copy. Implementers should work from the requirements in this pack and public standards, not from the reference product's artifacts.

## Public web sources

| ID | Source | URL | Type | Date / version | Reliability |
|---|---|---|---|---|---|
| S01 | Meld Studio docs index | https://meldstudio.co/docs/ | Official docs | "Last updated Oct 15, 2026" | High (vendor) |
| S02 | Meld docs: Clipping | https://meldstudio.co/docs/recording/clipping/ | Official docs | Undated; fetched 2026-10-26 | High (vendor) |
| S03 | Meld docs: Multistream | https://meldstudio.co/docs/outputs/multistream/ | Official docs | Undated; fetched 2026-10-26 | High (vendor) |
| S04 | Meld docs: Multi Canvas | https://meldstudio.co/docs/outputs/multi-canvas/ | Official docs | Undated; fetched 2026-10-26 | High (vendor) |
| S05 | Meld docs: Audio | https://meldstudio.co/docs/audio/ | Official docs | Undated; fetched 2026-10-26 | High (vendor) |
| S06 | Meld docs: Recording | https://meldstudio.co/docs/recording/recording/ | Official docs | Undated; fetched 2026-10-26 | High (vendor) |
| S07 | Meld docs: Spark | https://meldstudio.co/docs/spark/ | Official docs | Undated; fetched 2026-10-26 | High (vendor) |
| S08 | Meld docs: Meld Multi (Beta) | https://meldstudio.co/docs/meld-multi/ | Official docs | Undated; fetched 2026-10-26 | High (vendor) |
| S09 | Meld docs: API | https://meldstudio.co/docs/api/ | Official docs | Undated | High (vendor) |
| S10 | MeldStudio/streamdeck `WebChannelAPI.md` | https://github.com/MeldStudio/streamdeck/blob/main/WebChannelAPI.md | Official public API doc | Fetched 2026-10-26 | High (vendor) |
| S11 | Meld docs: Install | https://meldstudio.co/docs/getstarted/install/ | Official docs | Undated | High (vendor) |
| S12 | "Meld Studio vs OBS: The Complete 2026 Comparison" | https://meldstudio.co/blog/meld-studio-vs-obs-the-complete-2026-comparison/ | Vendor blog | 2026-02-19 | Medium (vendor marketing) |
| S13 | Release notes v0.9.3.4 (vertical replay clips) | https://meldstudio.co/blog/enhanced-media-layers-vertical-replay-clips-copy-paste-fix-v0-9-3-4-release-notes-2/ | Vendor release notes | v0.9.3.4 | High (vendor) |
| S14 | "Multi-Canvas Comes to Meld Studio" v0.9.0.0 | https://meldstudio.co/blog/multi-canvas-comes-to-meld-studio/ | Vendor release notes | v0.9.0.0 | High (vendor) |
| S15 | "Twitch Enhanced Broadcasting" v0.9.9.8 | https://meldstudio.co/blog/twitch-enhanced-broadcasting/ | Vendor release notes | v0.9.9.8 | High (vendor) |
| S16 | Meld transparency statement (Spark model) | https://meldstudio.co/transparency/ | Vendor statement | Undated | Medium |
| S17 | Amazon IVS Multitrack Video: Broadcast Software Integration Guide | https://docs.aws.amazon.com/ivs/latest/LowLatencyUserGuide/multitrack-video-sw-integration.html | Official third-party spec | Fetched 2026-10-26 | High |
| S18 | Enhanced RTMP v2 specification | https://veovera.org/docs/enhanced/enhanced-rtmp-v2 | Open spec | Referenced by S17 | High |
| S19 | OBS Studio 32.2 release notes | https://obsproject.com/blog/obs-studio-32-2-release-notes | Official OBS | 32.2 | High |
| S20 | Streamlabs: Twitch multiple encodes | https://streamlabs.com/content-hub/post/twitch-multiple-encodes-streamlabs-desktop | Third-party vendor | Undated | Medium |
| S21 | XSplit: Twitch Enhanced Broadcasting | https://support.xsplit.com/en/article/twitch-enhanced-broadcasting-17ajsco/ | Third-party vendor | Undated | Medium |
| S22 | Eklipse: StreamLadder alternatives | https://eklipse.gg/compare/streamladder-alternatives/ | Competitor marketing | 2026 | Low–medium |
| S23 | Eklipse vs StreamLadder | https://eklipse.gg/compare/eklipse-vs-streamladder/ | Competitor marketing | "updated July 2026" | Low–medium |
| S24 | Parse brand profile: meldstudio.co | https://www.parse.gl/brands/meldstudio-co | Third-party profile | April 2026 | Low |
| S25 | streamer.guide: Meld Spark | https://streamer.guide/blog/meld-spark-streaming-platform-2026 | Third-party blog | 2026-04-16 | Low |
| S26 | OBS forum: Twitch 2K beta thread | https://obsproject.com/forum/threads/i-got-access-to-twitch-2k-open-beta-1440p-60fps-but-it-doesnt-work.186554/ | Community forum | Undated | Low |

## Local, non-invasive observations

Target: the user's legitimately installed copy of Meld Studio, MSIX package `MeldStudioInc.MeldStudio`, version **0.10.7.7**, x64, inspected 2026-10-26. No process was attached, no code was executed, no authentication, licensing, DRM or encryption was touched, and no credentials or private data were extracted.

| ID | Method | What was examined | Result reference |
|---|---|---|---|
| L01 | File inventory + `AppxManifest.xml` read | Package file list, manifest, version resource | Version 0.10.7.7; Windows 10 build 19041+; `meld:` URL protocol; `meldstudio.exe` execution alias; auto-update through `.appinstaller` |
| L02 | REA `inspect_pe_resources` | `MeldStudio.exe` (SHA-256 `20365fa1…c209c3`) | Evidence `ev_f8dd18b2aab5efeff375a8bf7535f3413a450992903c4d8b6c34acbf8e0eedce`: only icons, version and manifest resources; no embedded UI resources in PE resource table |
| L03 | REA `inspect_rust_binary` | `MeldStudio.exe` | Evidence `ev_2f909fe9940248c2ab1deb3d698f5978d224dfb8712d989c8131babe98a5746a`: no cargo-auditable metadata, no rustc path. Absence does not prove no Rust |
| L04 | Local keyword scan of printable strings (Python script; not an REA tool) | `MeldStudio.exe` | Third-party component and feature-name presence only. See `unknowns-and-risks.md` §Static inspection findings |
| L05 | Reading shipped HTML shells | `test_recording_review.html`, `replay_clip.html` (title and loader only), `media_controller.html` | Web views talk to the native host through Qt WebChannel |
| L06 | User-owned session file, top-level key shape only | `%USERPROFILE%\MeldStudio\default\session.json` | Shape: audio tracks, scenes, vertical scenes, separate scene transitions per canvas, video capture devices. Contents not recorded |
| L07 | Archive file names | `Documents\MeldStudio\archives\*-before-<version>-<hash>.json` | Config migration snapshot taken before each upgrade, 0.9.11.2 → 0.10.7.7 |

## Background knowledge (not re-verified in this pass)

Items marked **[BK]** in other documents come from the researcher's general domain knowledge (for example OBS Studio's replay buffer, obs-websocket, FFmpeg capabilities, Whisper-class speech recognition). Verify them before relying on them for a design decision.
