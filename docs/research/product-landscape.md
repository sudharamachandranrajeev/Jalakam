# Product Landscape

Labels: **[O]** observation (cited), **[H]** hypothesis, **[R]** recommendation, **[BK]** background knowledge not re-verified.

## 1. Reference product: Meld Studio

### Positioning
- [O] Free streaming and recording app for Windows and macOS. It is not open source. The vendor says it ships updates every two weeks. (S12, S24)
- [O] Supported OS: 64-bit Windows 10 2004 (build 19041) or later, and macOS 12 or later. Linux is not available. (S01, S11, L01)
- [O] Installed build inspected: 0.10.7.7. It is still a pre-1.0 version number. (L01, L07)

### Core capabilities (vendor-documented)
| Area | What the docs say | Source |
|---|---|---|
| Scenes and layers | Scene templates, a layer stack per scene, widgets, 30+ real-time visual effects | S01, S12 |
| Multi Canvas | Optional second Portrait canvas, fixed at 1080×1920. Shared scenes, an independent layer stack per canvas, shared framerate, separate transitions, and a Duplicator layer for reusing portrait layouts | S04, S14 |
| Outputs | Native Twitch, YouTube, Kick and X outputs, plus custom RTMP/RTMPS. Outputs start and stop independently and can be added while live. Encoders are shared when settings match. An Encoder View shows the routing | S03 |
| Twitch Enhanced Broadcasting | Multitrack encoding done locally. Other outputs can use only the canvases included in the Twitch output | S03, S15 |
| Cloud relay | Meld Multi (beta) receives one RTMP stream and fans it out to Twitch, YouTube, Kick, Facebook, TikTok and custom RTMP. Free during beta | S08 |
| Recording | MP4, MOV, MKV and FLV. H.264 by default; HEVC and AV1 when the hardware supports them. Recording settings are independent of streaming. Multitrack audio is supported (MKV recommended) | S06 |
| Clipping | An always-on background encoder. Clip lengths of 10/30/60/90/120 s (90 by default). Choice of Main or Portrait canvas. Follows the stream settings | S02 |
| Replay Clip | Instant in-scene playback overlay with scrub, A/B loop, speed control and its own audio track | S02, S13 |
| Audio | Linked or per-scene tracks, process audio, Voice Clarity noise suppression, gain, compressor/ducking, panner, experimental VST3 support, CUE monitoring, and a Twitch VOD-excluded track | S05 |
| Virtual camera | Windows and macOS | S01 |
| AI | Meld Spark is a chat assistant that generates HTML/CSS/JS overlays and widgets. It runs server-side; the vendor says it uses Google Gemini Flash. It is optional and plan-limited | S07, S16 |
| Background effects | Background removal and blur | S01 |
| Integrations | Stream Deck, Streamer.bot, Lumia Stream, Mix It Up, Wave Link, BEACN, Spout2, Fourthwall, built-in multi-chat | S01 |
| Automation API | Qt WebChannel over WebSocket at `ws://127.0.0.1:13376`. Exposes scene, track, layer and effect control, stream/record toggles and commands such as record-clip | S09, S10 |

### Observed technology stack (static packaging inspection only)
- [O] Native C++ application with a Qt 6 / QML UI, Chromium Embedded Framework for browser sources, a bundled WebView2 runtime, FFmpeg, crash reporting through Sentry and Crashpad, the Discord Game SDK, and a virtual-camera driver installer. (L01)
- [O] Encoder paths named for NVENC (D3D11 and D3D12), x264, AMD AMF and Media Foundation. VST3 hosting and Spout2 shared textures are present. (L04)
- [H] The media pipeline appears to reuse Chromium media, Skia and D3D12 memory-allocator components inside a proprietary layer, rather than libobs. This is an inference from bundled component strings. It is not evidence of the actual architecture.
- [O] Auxiliary UIs (replay clip, recording review, media controller) are HTML pages hosted in web views that talk to native code through Qt WebChannel. (L05)
- [O] An importer for OBS scene collections exists. (L04)

### Not found in docs or packaging
- [O] No AI highlight detection, auto-clipping, transcript-based editing, or clip publishing to social platforms is documented. The clipping docs describe no trimming or editing beyond the replay overlay. (S02)
- [O] Speech-to-text and caption strings exist in the binary (L04), but this pass found no public docs page for them. Status unknown. See `unknowns-and-risks.md`.
- [O] No SRT, WHIP or WebRTC output strings were found in the binary. (L04) That negative search is bounded: only the strings were scanned.

## 2. Alternatives

| Product | Model | Relevant strengths | Relevant gaps | Source |
|---|---|---|---|---|
| OBS Studio | Free, GPL, Win/macOS/Linux | Largest plugin ecosystem; obs-websocket automation; replay buffer; multitrack video for Twitch EB; a WebRTC (WHIP) output was added in 32.1; dynamic bitrate for multitrack in 32.2 | Multistream and vertical output need plugins; replay buffer needs manual setup; no native clip editing | S12, S19, [BK] |
| Streamlabs Desktop | Freemium, built on OBS | Supports Twitch multiple encodes from 1.18.3 | EB is Windows-only and incompatible with multistream or dual output | S20 |
| XSplit Broadcaster | Commercial | Supports Twitch EB from 4.5.2406 | EB needs NVENC | S21 |
| Eklipse | SaaS AI clipper | Processes VODs into 10–20 timestamped highlights, 9:16 export, captions | Post-stream only and cloud-based; free tier capped at 720p | S22, S23 |
| StreamLadder | SaaS clip reformatter | Converts clips to 9:16 with captions and layout templates | Limited AI moment detection (sources disagree) | S22, S23 |
| Opus Clip | SaaS AI clipper | General long-form to short-form clipping | Not stream-native; details unverified | S22 |

## 3. Mature technical patterns worth adopting
- [O] **Automatic stream configuration.** Twitch/IVS multitrack expects the client to call GetClientConfiguration, use the returned encoder settings exactly, align IDRs across tracks, insert BPM SEI metrics, and reconnect with exponential backoff (OBS uses up to 25 retries, capped at 15 minutes). (S17, S18)
- [O] **Encoder sharing.** When outputs use the same canvas and settings, they share one encode instead of encoding twice. (S03)
- [O] **Config migration snapshots.** Prefs and session are archived before each version upgrade. (L07)
- [BK] **Ring-buffer replay.** OBS keeps encoded packets in memory and writes them out on demand, avoiding a re-encode. Meld appears to do the same with a background encoder. (S02)
- [O] **Local loopback automation API.** A local WebSocket API is how the Stream Deck and bot ecosystems integrate. (S10)

## 4. Gaps worth addressing (differentiation)
- [R] **Live AI moment detection.** Score moments during the stream from audio energy, chat velocity, platform events and transcript keywords. Nothing found in Meld or OBS does this in-app. Eklipse does it only after the stream. (S02, S22)
- [R] **Transcript-first clip editor.** Trim by editing text, add auto-captions, and reframe to 9:16 with face/subject tracking.
- [R] **Dual-canvas-native clips.** Clip the Portrait canvas directly; Meld already does this (S02). Add AI reframing of landscape-only footage when no Portrait canvas exists.
- [R] **Publishing pipeline.** Queue a review, then publish to YouTube Shorts, TikTok and Twitch clips with metadata drafted by AI. No reference product documents this.
- [R] **Linux support.** Meld has none. (S12)
