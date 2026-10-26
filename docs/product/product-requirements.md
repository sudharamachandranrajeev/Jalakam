# Product Requirements: Jalakam Release One

Jalakam is a Windows desktop streaming studio with an integrated AI editing and publishing workspace. A streamer composes scenes, captures audio and video, streams and records, then turns the best moments into platform-ready clips without leaving the app.

Inputs: the Phase 0 research pack, ADR 0001 (D1–D5) and `assumptions-and-decisions-needed.md` (A-xx). Detailed test cases are in `acceptance-criteria.md` (AC-xx). Journeys are in `user-journeys.md` (UJ-xx).

## Conventions

**Priority**
- **P0**: required for release one. The end-to-end journey or stream stability fails without it.
- **P1**: important, and planned for release one where the schedule allows. Otherwise it may slip to 1.1 without blocking release.
- **P2**: later, or experimental.

**Verification methods**
| Code | Method |
|---|---|
| UT | Unit test |
| IT | Integration test (real FFmpeg/GPU/device in CI or on a lab machine) |
| E2E | Automated UI end-to-end test (WebDriver against the Tauri app) |
| HW | Manual hardware-lab test on the reference and minimum machines (A-01, A-02) |
| SOAK | Long-duration run with metrics capture |
| FI | Fault injection (kill process, pull device, drop network, fill disk) |
| INSP | Design or code inspection |
| A11Y | Accessibility audit (keyboard pass, screen reader pass, contrast tool) |
| UX | Moderated usability test with streamers |

**Experimental flag**: requirements tagged *[Labs]* ship behind a Labs toggle that is off by default and are excluded from release-one stability gates.

Column order in every table: ID | User story | Priority | Acceptance criteria | Dependencies | Failure behaviour | Performance / reliability | Verification.

---

## 1. Onboarding, preferences and device discovery

| ID | User story | Pri | Acceptance criteria | Dependencies | Failure behaviour | Performance / reliability | Verify |
|---|---|---|---|---|---|---|---|
| ONB-001 | As a new user, I want a first-run wizard so I can go from install to a working scene in minutes | P0 | The wizard covers: hardware probe → canvas choice (landscape / landscape+portrait) → camera → microphone → template scene → recording folder. Every step can be skipped. Finishing produces a scene that previews camera + display. Completion is persisted and the wizard never reappears unless reset | ONB-002, SCN-001, CAN-002 | Any probe failure shows the device as "unavailable" with a reason and lets the user continue. The wizard never blocks app use | Wizard completes in < 3 min median (UX); the probe step finishes in < 5 s | E2E, UX |
| ONB-002 | As a user, I want the app to discover my GPUs, encoders, displays, cameras and audio devices so I only pick from things that work | P0 | Lists: GPU adapters with vendor/driver version; available encoders (NVENC H.264/HEVC/AV1, MF H.264, AMF/QSV if present); monitors with resolution/refresh; cameras with supported formats; audio input/output endpoints. Unsupported items are shown disabled with a reason | A-02, A-18 | A probe crash in one subsystem is isolated; others still report. Results are cached and re-probed on demand | Full probe < 5 s on reference HW | IT, HW |
| ONB-003 | As a user, I want devices plugged in or removed while the app runs to be handled gracefully | P0 | Adding a device makes it selectable within 3 s. Removing an in-use device marks its source "disconnected" and shows a placeholder. Reconnecting the same device resumes it automatically without a restart | ONB-002, AUD-009 | Never crashes; live outputs keep running with the placeholder frame or silence | No output interruption on hot-unplug | FI, HW |
| ONB-004 | As a user, I want preferences for paths, video, audio, hotkeys, outputs, privacy and appearance | P0 | Preferences are grouped by area. Changes validate inline. Settings that need an idle pipeline are disabled while live, with an explanation. Stored as versioned JSON (A-17) | A-17, SYS-010 | An invalid or corrupt prefs file falls back to defaults, backs up the bad file, and tells the user | Prefs load in < 100 ms | UT, E2E |
| ONB-005 | As a streamer, I want global hotkeys for start/stop stream, record, save clip, add marker and switch scene, so I can operate while gaming | P0 | Hotkeys work while another app (a full-screen game) has focus. Conflicts are detected at binding time. Defaults are unbound except save clip and add marker | LIB-004, LIB-005 | A failed hotkey registration is shown in the hotkey list with the reason (for example "in use by another app") | Hotkey-to-action latency < 50 ms | IT, HW |
| ONB-006 | As a new streamer, I want recommended encoder settings for my machine and connection | P1 | An optional bandwidth test against the chosen ingest plus an encoder capability check. It recommends resolution, fps, bitrate and encoder per output; the user accepts or edits | ONB-002, OUT-004 | If the test fails, it falls back to conservative defaults (720p30, 3500 kbps) and says so | Test < 30 s | IT, HW |
| ONB-007 | As a user, I want to connect a YouTube account and, optionally, a Twitch account | P1 | OAuth through the system browser with PKCE. Shows the connected account name. Revoke and disconnect are supported. Tokens are stored per SYS-006 | SYS-006, A-24, A-09 | Expired or revoked tokens prompt a re-connect and never fail silently | Token refresh happens in the background | IT, INSP |

## 2. Scene collections, scenes, sources, transforms, layers and transitions

| ID | User story | Pri | Acceptance criteria | Dependencies | Failure behaviour | Performance / reliability | Verify |
|---|---|---|---|---|---|---|---|
| SCN-001 | As a user, I want scene collections so I can keep separate setups (for example "Gaming", "Just Chatting") | P0 | Create, rename, duplicate, delete and switch collections. Export a collection to a single file and import it back with identical results (excluding missing media, which is flagged) | A-17 | A corrupt collection file opens read-only with a recovery prompt (SYS-002) | Switching collections < 2 s when idle; disallowed while live | UT, E2E |
| SCN-002 | As a user, I want to create, reorder, duplicate and delete scenes | P0 | Scene list operations update preview and program correctly. Deleting the live program scene is blocked | SCN-001 | n/a | Scene switch (cut) visible in the program within 1 frame | E2E |
| SCN-003 | As a user, I want to add display, window, camera, image, text and browser sources | P0 | Display and window capture via Windows Graphics Capture; camera via Media Foundation; image (PNG/JPG/WebP/GIF first frame); text (font, size, colour, outline); browser source (URL or local HTML, size, custom CSS, audio routed to the mixer) | ONB-002, VID-002 | A failed source shows an error badge and a placeholder; other sources unaffected; browser-source renderer crash restarts automatically (max 3 per minute) | Browser source at 1080p60 ≤ 1 CPU core on reference HW | IT, HW |
| SCN-004 | As a user, I want to add local video/audio media files as sources (looping, restart on show) | P1 | MP4/MKV/MOV/WebM playback through FFmpeg decode; loop; restart when shown; audio routed to the mixer | SCN-003 | A missing file shows a placeholder and keeps its path for relinking | Hardware decode where available | IT |
| SCN-005 | As a user, I want a layer stack per scene with order, visibility, lock and rename | P0 | Drag to reorder; toggle visibility; lock prevents move/resize; all changes reflected in preview within 1 frame | SCN-002 | n/a | n/a | E2E |
| SCN-006 | As a user, I want to group layers | P1 | Group, ungroup, transform the group as a unit, toggle group visibility | SCN-005 | n/a | n/a | E2E |
| SCN-007 | As a user, I want to position, scale, rotate and crop layers with snapping and fit/fill helpers | P0 | Handles in the preview, numeric fields, snap to canvas edges/centre/other layers, alt-drag crop, fit/fill/stretch/centre commands, reset transform | SCN-005, CAN-001 | Invalid numeric input is rejected inline | Interactive drag ≥ 60 fps in the preview | E2E, UT |
| SCN-008 | As a user, I want transitions between scenes | P0 | Cut and fade with configurable duration (0–2000 ms); a transition can be chosen per canvas (CAN-003) | SCN-002 | n/a | No dropped output frames during a transition on reference HW | IT, HW |
| SCN-009 | As a user, I want stinger and move transitions | P2 | Stinger video with cut point; move transition interpolates matched layers | SCN-008, SCN-004 | n/a | n/a | IT |
| SCN-010 | As a user, I want preview/program (studio) mode so I can stage a scene before taking it live | P1 | Separate preview and program views; "take" applies the selected transition | SCN-008 | n/a | Studio mode adds ≤ 10 % GPU load on reference HW | E2E, HW |
| SCN-011 | As a user, I want undo/redo for scene edits | P0 | At least 100 steps per collection for add/remove/reorder/transform/property changes; Ctrl+Z / Ctrl+Y | SCN-005, SCN-007 | Undo history is cleared, with a notice, when a collection is switched | n/a | UT, E2E |
| SCN-012 | As a user, I want basic layer effects: chroma key, colour correction, opacity | P1 | Per-layer effect stack, reorderable; parameters editable live | SCN-005 | A failed effect shader disables that effect only and flags it | Each effect ≤ 0.5 ms GPU at 1080p on reference HW | IT, HW |

## 3. Microphone, desktop/game audio, monitoring, mixing and filters

| ID | User story | Pri | Acceptance criteria | Dependencies | Failure behaviour | Performance / reliability | Verify |
|---|---|---|---|---|---|---|---|
| AUD-001 | As a streamer, I want to capture my microphone | P0 | Select any WASAPI capture endpoint; mono/stereo; 48 kHz internal mix with resampling of any device rate | ONB-002 | Device loss: see AUD-009 | Capture-to-mix latency ≤ 20 ms | IT, HW |
| AUD-002 | As a streamer, I want desktop/game audio captured | P0 | WASAPI loopback of any render endpoint; default-device follow mode tracks Windows default changes | ONB-002 | Endpoint change without a glitch longer than 100 ms | ≤ 20 ms added latency | IT, HW |
| AUD-003 | As a streamer, I want to capture a single application's audio (for example only the game, not Discord) | P1 | Pick a running process; captures only that process tree; hidden on unsupported Windows builds (A-19) | A-19 | When the process exits, the track goes silent and reattaches when the process restarts | ≤ 20 ms added latency | IT, HW |
| AUD-004 | As a user, I want a mixer with per-track volume, mute and meters | P0 | Fader in dB (−∞ to +20 dB); mute; peak + RMS meters at ≥ 30 Hz refresh; clip indicator; tracks can be renamed and reordered | AUD-001/002 | n/a | Meter UI updates must not block the audio thread | UT, E2E |
| AUD-005 | As a user, I want integrated loudness (LUFS) metering on the master output | P1 | Momentary, short-term and integrated LUFS per EBU R128 | AUD-004 | n/a | n/a | UT |
| AUD-006 | As a streamer, I want to monitor tracks through headphones | P0 | Per-track monitor off / monitor only / monitor and output; selectable monitoring device | AUD-004 | A lost monitoring device falls back to silent monitoring with a notice; outputs are unaffected | Monitoring latency ≤ 40 ms | HW |
| AUD-007 | As a streamer, I want gain and noise suppression filters | P0 | Per-track filter chain; gain (±30 dB); noise suppression (RNNoise-class, permissive licence) with on/off and strength | AUD-004, D2 | A filter that fails to initialise is bypassed and flagged; audio still flows | Noise suppression ≤ 3 % of one core per track | UT, IT |
| AUD-008 | As a streamer, I want a noise gate, compressor and limiter | P1 | Standard parameters (threshold, ratio, attack, release, makeup gain) | AUD-007 | Same as AUD-007 | Same as AUD-007 | UT |
| AUD-009 | As a streamer, I want audio device loss handled without breaking my stream | P0 | Unplugging an in-use device inserts silence for that track and shows "disconnected"; replugging the same device reattaches it within 3 s | ONB-003 | Never stalls the mix clock | Master mix continues without gaps | FI |
| AUD-010 | As a user, I want per-track sync offset to align audio with video | P0 | Offset from −500 to +2000 ms in 1 ms steps, applied consistently to stream, recording and replay | AUD-004, VID-010 | n/a | n/a | IT |
| AUD-011 | As a streamer, I want to choose which tracks go to the stream, the recording and clips | P1 | Routing matrix: track × {stream mix, recording track 1–6, clip mix}. Multitrack recording writes separate audio tracks in MKV | AUD-004, VID-006 | n/a | n/a | IT |
| AUD-012 | As a streamer, I want ducking (lower music when I speak) | P2 | Sidechain compressor keyed from a chosen track | AUD-008 | n/a | n/a | UT |

## 4. Video capture, compositing, encoding, streaming and recording

| ID | User story | Pri | Acceptance criteria | Dependencies | Failure behaviour | Performance / reliability | Verify |
|---|---|---|---|---|---|---|---|
| VID-001 | As a user, I want to choose canvas resolution and frame rate | P0 | Landscape resolutions up to 1920×1080; 24/25/30/50/60 fps (A-03); change only while idle | A-03 | n/a | n/a | E2E |
| VID-002 | As a streamer, I want capture and compositing to stay on the GPU so my game keeps its frame rate | P0 | Captured frames stay as GPU textures from capture through compositor to encoder (no CPU readback on the NVENC path) | D3, VID-003 | When zero-copy is unavailable (for example a cross-adapter capture), falls back to a copy path and logs the reason | Compositor ≤ 2 ms GPU per frame at 1080p60 for a typical scene (camera + display + 2 browser sources) on reference HW | IT, HW, INSP |
| VID-003 | As a streamer, I want hardware encoding with a software fallback | P0 | NVENC H.264 (P0); Media Foundation H.264 software fallback (P0); NVENC HEVC/AV1 (P1); AMF/QSV (P1, A-18). Rate control CBR/VBR; keyframe interval (default 2 s); presets | ONB-002, D2 | If an encoder fails to start or crashes mid-session, the affected output stops with a clear error; recording and other outputs continue; the user is offered the fallback | Encoder-skipped frames < 0.1 % (NFR-PERF-01) | IT, HW, SOAK |
| VID-004 | As a streamer, I want to stream over RTMP/RTMPS | P0 | Custom server URL + stream key; RTMPS with TLS certificate validation; URL validation (scheme required); FCUnpublish on clean stop (Phase 0 S17) | VID-003, SYS-006 | A connection failure shows a specific error (DNS, TLS, auth rejected, timeout) | Time from Start to first packet sent < 3 s | IT, HW |
| VID-005 | As a streamer, I want automatic reconnection when my network drops | P0 | Exponential backoff with jitter (base 2 s, cap 15 min, at most 25 attempts, matching common practice per Phase 0 S17); user can cancel; recording continues during reconnect | VID-004 | After the final attempt, the output enters an "Ended – network" state with a summary | Reconnect within 5 s once the network returns (first-attempt window) | FI |
| VID-006 | As a user, I want local recording that survives crashes | P0 | Default MKV with remux to MP4 on stop (A-10); choose canvas, encoder, bitrate/quality and audio tracks independently of streaming; file naming template with date/time | VID-003, A-10, A-22 | Disk full: stop the recording cleanly, finalise the file, alert (LIB-007). Crash: file recoverable (SYS-002) | Recording loses ≤ 2 s on process kill (NFR-REL-02) | FI, IT |
| VID-007 | As a streamer, I want to record and stream at the same time with different settings | P0 | Recording and stream outputs are independent; encoder sharing applies only when settings are identical (OUT-002) | VID-004, VID-006, OUT-002 | A failure in one does not stop the other | Both running meets NFR-PERF-01 on reference HW | SOAK, HW |
| VID-008 | As a streamer, I want live health stats | P0 | Per output: bitrate, dropped (network) frames, encoder-skipped frames, compositor-lagged frames, RTT (where available), uptime; global CPU/GPU/encoder utilisation; warnings on thresholds | VID-003/004 | n/a | Stats collection ≤ 0.5 % of one CPU core | IT, E2E |
| VID-009 | As a streamer, I want the app to warn me before overload breaks my stream | P1 | Detect sustained encoder overload (> 1 % skipped over 10 s) and offer one-click actions: lower preset, lower output resolution | VID-008 | Never changes settings without the user's consent | n/a | HW |
| VID-010 | As a viewer, I want audio and video to stay in sync for the whole stream | P0 | Single master clock; every frame and audio buffer timestamped against it; A/V offset in the output ≤ 20 ms at start and ≤ 30 ms after 4 h | AUD-010 | Drift beyond 40 ms logs a warning and resynchronises at the next keyframe | NFR-PERF-03 | SOAK (clapper test) |

## 5. Multiple stream destinations

| ID | User story | Pri | Acceptance criteria | Dependencies | Failure behaviour | Performance / reliability | Verify |
|---|---|---|---|---|---|---|---|
| OUT-001 | As a streamer, I want to stream to up to 3 destinations at once | P0 | Up to 3 RTMP/RTMPS outputs (D5) configured and running at once; each shows its own status | VID-004, D5 | A fourth output cannot be added; the UI explains the limit | Three 1080p60 outputs on reference HW meet NFR-PERF-01 when they share one encoder | SOAK, HW |
| OUT-002 | As a streamer, I want each destination to have its own output profile (canvas, resolution, bitrate, encoder) where my hardware allows | P0 | Profile per output. Outputs with identical encoder settings share one encode automatically. The UI shows the encoder graph (which outputs share which encoder) and the total encoder sessions in use vs available | OUT-001, CAN-005 | If a new profile would exceed encoder sessions or GPU budget, it is rejected before going live with the reason | Encoder sharing verified by encoder-session count | IT, INSP |
| OUT-003 | As a streamer, I want to start and stop each destination independently | P0 | "Start all", "Stop all" and per-output start/stop; stopping one never interrupts others | OUT-001 | n/a | No frame drops on other outputs during a start/stop | FI, SOAK |
| OUT-004 | As a streamer, I want a pre-flight check before going live | P0 | Checks: total bitrate vs measured or entered upload bandwidth (warn above 80 %), encoder session count, disk space for recording, missing sources, connected accounts | OUT-002, LIB-007 | Warnings can be overridden; errors (for example invalid URL) block the start | Check < 2 s | E2E |
| OUT-005 | As a streamer, I want to add a destination while already live | P1 | A new output can start mid-session if it can share an existing encoder or capacity is available | OUT-002 | Rejected with the reason if capacity is unavailable | No drops on existing outputs | FI |
| OUT-006 | As a streamer, I want platform presets (Twitch, YouTube, Kick, custom) that warn about platform limits | P1 | The preset fills in the ingest URL list and keyframe/bitrate guidance, stored as dated data (A-12) | A-12 | Values outside guidance warn and do not block | n/a | UT |
| OUT-007 | As a streamer, I want Twitch Enhanced Broadcasting (multitrack) | Out of scope R1 | Recorded so it is not accidentally built (Phase 0 F13, U09) | — | — | — | — |

## 6. Landscape and portrait canvases

| ID | User story | Pri | Acceptance criteria | Dependencies | Failure behaviour | Performance / reliability | Verify |
|---|---|---|---|---|---|---|---|
| CAN-001 | As a streamer, I want a landscape main canvas | P0 | Always present; resolution and fps per VID-001 | VID-001 | n/a | n/a | E2E |
| CAN-002 | As a streamer, I want an optional portrait canvas (1080×1920) for vertical platforms and clips | P0 | Enable/disable in preferences while idle; shares the frame rate with the main canvas | A-03 | Disabling keeps portrait layouts stored for re-enabling | Dual canvas adds ≤ 1.5 ms GPU per frame vs single canvas on reference HW | HW |
| CAN-003 | As a streamer, I want each scene to have separate layer stacks per canvas, switched together | P0 | Scene switch changes both canvases at once; each canvas has its own layers and its own transition selection; the editor clearly shows which canvas is being edited (colour plus text label, not colour alone) | CAN-002, SCN-005, SYS-008 | n/a | n/a | E2E |
| CAN-004 | As a user, I want to copy a layer from one canvas to the other | P1 | Copy keeps the source reference (same camera device, same browser URL) with a fresh transform fitted to the target canvas | CAN-003 | n/a | Shared device sources are captured once and used by both canvases | IT |
| CAN-005 | As a streamer, I want every output, recording and replay buffer to target a chosen canvas | P0 | Canvas selector on each output profile, recording settings and replay buffer | CAN-002, OUT-002, LIB-003 | Disabling a canvas that an active configuration uses is blocked while idle with a list of the dependants | n/a | E2E |
| CAN-006 | As a streamer, I want canvas configuration protected while live | P0 | Enable/disable portrait, change resolution or fps are disabled while any output, recording or replay buffer is active, with an explanation | CAN-002 | n/a | n/a | E2E |
| CAN-007 | As a streamer, I want to see both canvases while editing and while live | P0 | Side-by-side or tabbed preview; a live indicator per canvas | CAN-002 | n/a | Preview rendering never blocks output rendering | E2E, HW |

## 7. Media library, recording history, instant replay and clip creation

| ID | User story | Pri | Acceptance criteria | Dependencies | Failure behaviour | Performance / reliability | Verify |
|---|---|---|---|---|---|---|---|
| LIB-001 | As a creator, I want a media library of my recordings, clips and exports | P0 | Indexed list with thumbnail, date, duration, canvas, resolution, size and type; filter by type/date; search by title and (if available) transcript text; open in editor; reveal in Explorer | A-22 | Missing files are shown as "missing" with a relink option; the library index rebuilds from disk if corrupt | Library of 1,000 items opens in < 1 s | IT, E2E |
| LIB-002 | As a creator, I want a recording history that remembers what happened in each session | P0 | Each recording has a session record: start/end, outputs used, canvases, markers, clip saves, health stats summary and a sidecar event log (JSON Lines) of markers, scene switches, chat-rate samples (if connected) and stream events | VID-006, LIB-005 | The sidecar is written incrementally, so a crash leaves a valid log up to the last event | Sidecar writes are batched (≤ 1 write/s) | IT, FI |
| LIB-003 | As a streamer, I want a replay buffer that always holds the last N seconds | P0 | Per-canvas ring buffer of encoded packets (no re-encode on save); length 10–120 s (default 60); memory use shown; runs whether or not live | CAN-005, VID-003 | When memory is insufficient, it refuses to start with the required amount shown | Buffer uses ≤ (bitrate × length) + 10 % RAM; zero added GPU cost when sharing an existing encoder | IT, SOAK |
| LIB-004 | As a streamer, I want to save a clip from the buffer with a hotkey, button or API call | P0 | Save writes the last N seconds (configurable per save via API) starting at a keyframe, as MP4, to the clips folder, within 2 s; a toast confirms with "Open in editor" | LIB-003, ONB-005 | Disk full or write error: the toast shows the error; the buffer keeps running | Save does not cause output frame drops | IT, FI, HW |
| LIB-005 | As a streamer, I want to drop a marker during a live session without saving a clip | P0 | Hotkey or button adds a timestamped marker (optional label) to the session's sidecar log; markers appear on the recording's timeline in the editor | LIB-002, ONB-005 | n/a | < 50 ms from hotkey to marker | IT |
| LIB-006 | As a streamer, I want to play a saved clip back on stream as an instant replay | P2 | Replay layer plays the clip in-scene with auto-hide | LIB-004, SCN-004 | n/a | n/a | E2E |
| LIB-007 | As a user, I want the app to protect me from running out of disk | P0 | Free space checked before start and every 10 s while recording; warns at < 10 GB or < 30 min at the current bitrate; stops recording cleanly at < 1 GB | VID-006 | Finalises the file before stopping | n/a | FI |
| LIB-008 | As a creator, I want to import an existing video file into the library to edit it | P1 | Import MP4/MKV/MOV; probe; thumbnail; available to the editor and transcription | LIB-001 | Unsupported codecs are rejected with the codec name | n/a | IT |

## 8. AI-assisted highlight discovery, transcription, captioning and reframing

| ID | User story | Pri | Acceptance criteria | Dependencies | Failure behaviour | Performance / reliability | Verify |
|---|---|---|---|---|---|---|---|
| AI-001 | As a creator, I want my recordings and clips transcribed locally with word timestamps | P0 | After a recording or clip is saved (automatic, or on demand), a local ASR job produces a transcript with per-word timestamps and a detected language. Transcripts are stored next to the media and searchable (LIB-001) | A-06, AI-008, AI-007 | Job failure is shown in the job list with retry; media is unaffected | Transcribes 1 h of audio in ≤ 15 min on reference HW (GPU) or ≤ 60 min (CPU); never runs at above-normal priority while live (AI-007) | IT, HW |
| AI-002 | As a creator, I want captions generated from the transcript for my clips | P0 | Caption track per clip: segments of ≤ 2 lines, ≤ 42 chars per line, editable text and timing; export as burned-in (styled) and as SRT/VTT sidecar | AI-001, TL-004 | Missing transcript: offer to transcribe; captions can always be typed manually | Caption timing within ±100 ms of word timestamps | UT, IT |
| AI-003 | *[Labs]* As a streamer, I want suggested highlights after a session, ranked with reasons | P1 | Scores windows of the recording using: markers, audio loudness/excitement peaks, transcript keywords/laughter, chat-message rate (A-09) and stream events. Shows the top-N suggestions with signals and confidence (AI-009); accept → creates a clip project (TL-001) | AI-001, LIB-002, A-09, A-21 | No signals available: falls back to markers and loudness only, and says so | Scoring 4 h of session < 5 min after transcription; precision@10 ≥ 0.4 (A-21) | IT, eval harness |
| AI-004 | *[Labs]* As a streamer, I want live transcription and live highlight hints during the stream | P2 | Rolling transcript and a "moment detected" hint panel while live | AI-001, AI-007, A-04 | Auto-disables when output health degrades (VID-008 thresholds) | Must not raise skipped/lagged frames above the no-AI baseline | SOAK, HW |
| AI-005 | *[Labs]* As a creator, I want landscape footage auto-reframed to 9:16 by following the speaker or subject | P2 | Per-variant auto-track generates smoothed crop keyframes, which stay editable (TL-005) | A-07, TL-005 | Low-confidence segments fall back to centre crop and are flagged | Analysis ≤ 1× clip duration on reference HW | IT, UX |
| AI-006 | *[Labs]* As a creator, I want a title, description and hashtags drafted for each clip | P1 | Opt-in; uses the user's own API key (A-08); sends only the transcript text and user-entered context (never media); the draft is fully editable; the user sees what will be sent | A-08, D4, SYS-007 | No key or a network error: fields stay empty with a message; the publish flow continues manually | Response < 10 s | IT, INSP |
| AI-007 | As a streamer, I want AI work never to harm my live stream | P0 | Resource governor: while any output is live, AI and render jobs run at low priority with capped threads and pause when health warnings appear (VID-008); the user can pause all jobs | VID-008 | Paused jobs resume automatically when idle | NFR-PERF-01 holds with jobs queued | SOAK |
| AI-008 | As a user, I want AI models managed transparently | P0 | Models are downloaded on first use with size shown and consent; SHA-256 verified; stored per A-22; work fully offline after download; model version recorded in each transcript | A-06, A-22 | A checksum mismatch deletes the file and retries; offline with no model shows how to get it | Resumable download | IT |
| AI-009 | *[Labs]* As a creator, I want to know why a moment was suggested | P1 | Each suggestion lists contributing signals with values (for example "chat rate 4.1× baseline", "marker", "loudness +9 dB") | AI-003 | n/a | n/a | UX |
| AI-010 | *[Labs]* As the product team, we want accept/reject feedback on suggestions, stored locally, to improve scoring | P1 | Accept, reject and edit-distance stored locally; exported only via opt-in telemetry (A-13) | AI-003, SYS-007 | n/a | n/a | INSP |

## 9. Non-destructive timeline with editable cuts and platform variants

| ID | User story | Pri | Acceptance criteria | Dependencies | Failure behaviour | Performance / reliability | Verify |
|---|---|---|---|---|---|---|---|
| TL-001 | As a creator, I want to start a clip project from a saved clip, a marker, a suggestion or any range of a recording | P0 | "New clip project" from each entry point; the default range for a marker is marker −30 s to +15 s (editable) | LIB-001, LIB-004, LIB-005 | Source media missing: project opens with offline media and relink | Project opens in < 2 s for a 4 h source | E2E |
| TL-002 | As a creator, I want to trim, split and delete segments with the source untouched | P0 | Set in/out, split at playhead, delete segment (ripple), rearrange segments; snapping to word boundaries when a transcript exists | TL-001, AI-001 | n/a | Edits apply in < 100 ms | UT, E2E |
| TL-003 | As a creator, I want edits stored as an edit decision list so I can always change my mind | P0 | Project = versioned JSON (A-17) referencing source media by path + hash; no source file is ever modified; projects autosave (SYS-001) | A-17, SYS-001 | A changed source hash warns before render | n/a | UT, INSP |
| TL-004 | As a creator, I want independent platform variants of the same clip | P0 | One project holds variants (16:9 and 9:16 in P0; 1:1 in P1). Each variant has its own crop/reframe, caption style and position, optional extra trim and export preset. Shared cuts can be unlinked per variant | TL-002, EXP-002 | n/a | n/a | UT, E2E |
| TL-005 | As a creator, I want to frame each variant with presets or manual keyframes | P0 presets / P1 keyframes | P0: fit, fill, left/centre/right third crop presets, plus a manual static crop. P1: keyframed pan/zoom with easing | TL-004 | n/a | Preview updates in real time at 1080p | E2E |
| TL-006 | As a creator, I want the 9:16 variant to use my portrait-canvas footage when it exists | P0 | If a replay clip or recording exists for the portrait canvas over the same time range, the 9:16 variant offers it as the source, time-aligned to the landscape cut | CAN-002, LIB-002 | Different durations or offsets are aligned by the session clock; when that is impossible, a warning appears and a reframed landscape source is used | Alignment within 1 frame | IT |
| TL-007 | As a creator, I want to edit by transcript (delete words to cut them) | P1 | Selecting words in the transcript and deleting creates cuts; cuts show on the timeline; undoable | AI-001, TL-002 | n/a | n/a | E2E, UX |
| TL-008 | As a creator, I want smooth preview playback and accurate scrubbing | P0 | Playback at source frame rate for 1080p60 H.264 sources; seek lands on the requested frame (±0 frames on pause) | TL-001 | Decode error: placeholder for the affected range and an error badge | Seek < 150 ms on reference HW | IT, HW |
| TL-009 | As a creator, I want per-track audio control in the editor | P1 | Mute and gain per audio track of the source; normalise to target LUFS on export | AUD-011, EXP-002 | n/a | n/a | IT |
| TL-010 | As a creator, I want undo/redo in the editor | P0 | ≥ 200 steps per project, persisted across autosave | TL-002 | n/a | n/a | UT |
| TL-011 | As a creator, I want text titles and an image watermark on a variant | P2 | Text and image overlays with position and timing | TL-004 | n/a | n/a | E2E |

## 10. Export queue, validation, publishing and analytics feedback

| ID | User story | Pri | Acceptance criteria | Dependencies | Failure behaviour | Performance / reliability | Verify |
|---|---|---|---|---|---|---|---|
| EXP-001 | As a creator, I want an export queue that renders in the background | P0 | Add one or many variants; shows progress/ETA; pause, cancel, retry, reorder; survives an app restart (unfinished jobs restart) | TL-004, AI-007 | A failed job keeps its log and can be retried; other jobs continue | Queue state persisted after each job state change | IT, FI |
| EXP-002 | As a creator, I want export presets for YouTube, Shorts/TikTok/Reels and a generic master | P0 | Presets: 16:9 1080p and 9:16 1080×1920 using A-11; custom preset editing; presets are data files with a `checked_on` date (A-12) | A-11, A-12 | n/a | n/a | UT |
| EXP-003 | As a creator, I want every export validated so I never upload a broken file | P0 | After render: probe container/codec/profile/resolution/fps/duration; full decode pass with zero errors; A/V offset ≤ 1 audio frame; duration matches the EDL within ±1 frame; preset platform limits (A-12) checked; result shown as pass/fail with reasons | EXP-001, A-12 | Fail: the file is kept but marked invalid; publishing is blocked for it | Validation ≤ 20 % of render time | IT |
| EXP-004 | As a creator, I want fast hardware-accelerated rendering with a software fallback | P0 | NVENC render when available, otherwise Media Foundation; captions burned in during render | VID-003, AI-002 | Hardware failure retries once in software automatically | 60 s 1080p clip renders in ≤ 30 s on reference HW (hardware path) | IT, HW |
| EXP-005 | As a creator, I want to publish a validated clip to YouTube | P1 (gated, A-05) | Resumable upload via the YouTube Data API; title/description/tags (AI-006 draft optional), privacy default **Private** with Unlisted/Public choices; an explicit "Publish" confirmation per upload; Shorts eligibility hint for 9:16 | ONB-007, EXP-003, A-05, A-24 | Network loss resumes the upload; quota/auth errors are shown with the API message; never auto-retries a rejected upload | Upload resumes from the last confirmed chunk | IT, HW |
| EXP-006 | As a creator, I want to see the status of my publishes | P1 | Per item: queued, uploading %, processing, published (with link), failed (with reason) | EXP-005 | n/a | n/a | E2E |
| EXP-007 | As a creator, I want performance feedback (views, watch time) on published clips, and the AI to learn from it | P2 | Pulls YouTube Analytics for clips published from Jalakam; shows them in the library; optional feedback into the scoring eval set | EXP-005, AI-010 | API unavailable: the field shows "unavailable" | Polling ≤ once per hour per item | IT |
| EXP-008 | As a creator, I want TikTok, Twitch or other platform publishing | Out of scope R1 | Export presets cover these platforms; manual upload | D5 | — | — | — |

## 11. Crash recovery, autosave, logging, permissions, privacy and accessibility

| ID | User story | Pri | Acceptance criteria | Dependencies | Failure behaviour | Performance / reliability | Verify |
|---|---|---|---|---|---|---|---|
| SYS-001 | As a user, I want my work saved automatically | P0 | Scene collections save on every change (debounced 2 s); editor projects every 10 s and on blur; atomic write (temp + rename) with the last 10 versions kept | A-17 | Write failure: banner + retry; previous version stays intact | No UI stall > 16 ms from autosave | UT, FI |
| SYS-002 | As a user, I want recovery after a crash | P0 | On restart after an abnormal exit: offer to restore the last session; recover in-progress recordings (MKV, or fMP4 repair) and register them in the library; resume the export queue | VID-006, EXP-001, SYS-001 | A recovery that fails leaves the original files untouched and reports which ones | ≤ 2 s of recording lost (NFR-REL-02) | FI |
| SYS-003 | As a user, I want crash reports that help fix bugs without leaking my data | P0 local / P1 upload | Crash handler writes a minidump + recent log locally; upload only with consent (A-13); stream keys, tokens, file paths under the user profile and chat text are redacted | A-13, SYS-006 | The crash handler runs out of process | n/a | FI, INSP |
| SYS-004 | As a user or support engineer, I want useful logs | P0 | Structured logs, rotating (10 × 10 MB); levels; "Export diagnostics" creates a zip of logs, system info and settings with secrets redacted | SYS-006 | Logging failure never crashes the app | Logging ≤ 1 % CPU at the default level | UT, INSP |
| SYS-005 | As a user, I want clear help when Windows blocks my camera or microphone | P0 | Detects Windows privacy-setting denial for camera/mic; shows which setting to change with a button that opens the Settings page | ONB-002 | n/a | n/a | HW |
| SYS-006 | As a streamer, I want my stream keys and account tokens kept secret | P0 | Stored only in Windows Credential Manager / DPAPI; never in JSON config, logs, crash reports, exports or the clipboard history; masked in the UI with an explicit reveal | — | If the vault is unavailable, the user re-enters the key each session; plaintext fallback is never used | n/a | INSP, IT |
| SYS-007 | As a user, I want my media and data to stay on my PC unless I choose otherwise | P0 | No network calls except: configured stream ingests, update check, model download, opted-in telemetry/crash upload, user-initiated publish, opted-in LLM metadata (AI-006), connected-account APIs. A privacy page lists these and their state | D4, A-13 | n/a | n/a | INSP, network capture test |
| SYS-008 | As a user with accessibility needs, I want to operate the app by keyboard and screen reader | P0 baseline / P1 full | P0: every live control (start/stop, record, clip, marker, scene switch, mute) reachable and operable by keyboard with visible focus and accessible names; status never conveyed by colour alone; UI scale 100–200 %. P1: WCAG 2.2 AA across non-preview UI (A-14) | A-14 | n/a | n/a | A11Y |
| SYS-009 | As a power user, I want a local automation API for Stream Deck and bots | P1 | Localhost-only JSON-RPC over WebSocket; per-client token; commands: switch scene, start/stop outputs, record, save clip, add marker, mute; event stream of state changes | A-20, SYS-006 | Invalid token: connection refused; rate limit of 50 requests/s per client | Command latency < 50 ms | IT, INSP |
| SYS-010 | As a user, I want upgrades never to break my setup | P0 | Config and project schemas are versioned; before migrating, a snapshot of prefs and collections is archived (Phase 0 L07 pattern); downgrade-safe read of unknown fields | A-17 | A failed migration restores the snapshot and opens the previous state read-only with an explanation | n/a | UT |
| SYS-011 | As a user, I want a trustworthy install and updates | P0 installer / P1 auto-update | Signed installer (A-16); per-user install without admin where possible; auto-update with a signed manifest; updates never apply while live | A-16 | A failed update rolls back to the previous version | n/a | HW |
| SYS-012 | As the project, we must ship correct third-party licence notices | P0 | About → Licences lists every bundled dependency and licence; FFmpeg build configuration published as required by LGPL; CI fails on a GPL-only dependency (D2) | D2 | n/a | n/a | INSP, CI |

---

## 12. Cross-cutting non-functional requirements

| ID | Requirement | Pri | Acceptance criteria | Dependencies | Failure behaviour | Implication | Verify |
|---|---|---|---|---|---|---|---|
| NFR-PERF-01 | Frame delivery | P0 | On reference HW (A-01), with 1080p60 landscape + portrait canvases, 2 outputs, recording and replay buffer: compositor-lagged + encoder-skipped frames < 0.1 % over 4 h. Network-dropped frames are reported separately | A-01 | Health warnings (VID-008) | Drives the GPU-resident architecture (VID-002) | SOAK |
| NFR-PERF-02 | Game impact | P0 | Running Jalakam with the NFR-PERF-01 setup reduces a GPU-bound benchmark game's average FPS by ≤ 10 % vs not running | A-01 | — | Compositor and encode budget | HW |
| NFR-PERF-03 | A/V sync | P0 | ≤ 20 ms offset at start, ≤ 30 ms after 4 h, measured by clapper test on the recording and the stream VOD | VID-010 | Resync at keyframe | Single master clock | SOAK |
| NFR-PERF-04 | Startup | P0 | Cold start to interactive UI ≤ 5 s on reference HW; preview visible ≤ 8 s | — | — | Lazy-load AI and browser sources | HW |
| NFR-PERF-05 | Idle footprint | P1 | With the app open, not live and replay buffer off: ≤ 3 % CPU, ≤ 600 MB RAM | — | — | — | HW |
| NFR-REL-01 | Long-session stability | P0 | 4 h soak at the NFR-PERF-01 setup: zero crashes, zero output stalls > 1 s, memory growth ≤ 5 % after the first 15 min | — | — | Leak detection in CI soak | SOAK |
| NFR-REL-02 | Recording durability | P0 | Process kill (`taskkill /F`) during recording loses ≤ 2 s; the file plays in a standard player after recovery | SYS-002 | — | MKV / fragmented writes | FI |
| NFR-REL-03 | Fault isolation | P0 | Failure of one source, browser renderer, encoder session or output never terminates other outputs or the app | — | — | Process or thread isolation for browser sources and encoders | FI |
| NFR-SEC-01 | Secrets handling | P0 | No secret is present in any file under app data, logs or crash dumps (automated scan in CI and the release checklist) | SYS-006 | — | — | INSP, IT |
| NFR-SEC-02 | Network surface | P0 | No listening sockets by default; the automation API (P1) binds only to 127.0.0.1 with a token | SYS-009 | — | — | INSP |
| NFR-COMP-01 | OS and hardware coverage | P0 | Passes the release test matrix: Windows 10 22H2 and Windows 11 latest; NVIDIA RTX 20/30/40-series; one no-NVENC machine (software fallback) | D1, A-02 | — | Lab hardware needed | HW |
| NFR-MNT-01 | Licence compliance | P0 | CI licence scan passes (SYS-012) | D2 | Build fails | — | CI |
| NFR-UX-01 | Learnability | P1 | In moderated tests, 4 of 5 first-time streamers complete the golden journey (UJ-00) without help in ≤ 15 min | — | — | — | UX |
