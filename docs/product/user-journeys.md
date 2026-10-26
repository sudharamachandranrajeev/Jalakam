# User Journeys

Each journey lists its steps, the requirement IDs it exercises, and its failure branches. UJ-00 is the release-one golden journey that the MVP must demonstrate end to end.

## Personas

| Persona | Description | Primary goals |
|---|---|---|
| **P-A Growing streamer** | Streams 3–5 times a week to Twitch and YouTube; one gaming PC with an RTX-class GPU; edits clips by hand today in a separate editor | Stable stream; vertical clips without extra tools; less time editing |
| **P-B New streamer** | First streaming app; unsure about bitrates and encoders | Get live quickly with sensible settings |
| **P-C Creator-editor** | Records long sessions (podcasts, IRL) more than streaming; publishes Shorts and long-form videos | Fast transcript-driven editing; captions; reliable exports |

---

## UJ-00 Golden journey (release-one must-pass)

Persona P-A on reference hardware (A-01). It covers: configure a scene, capture audio and video, record or stream, select a highlight, edit a clip, render it, and export a valid file.

| Step | User action | System behaviour | Requirements |
|---|---|---|---|
| 1 | Installs and launches Jalakam | Signed installer; first-run wizard starts | SYS-011, ONB-001 |
| 2 | Accepts the hardware probe, chooses "Landscape + Portrait" | GPUs, NVENC, monitors, camera and audio devices are listed; portrait canvas enabled | ONB-002, CAN-002 |
| 3 | Picks the webcam and microphone; chooses a template | Scene created with display capture + camera on both canvases; mic and desktop audio tracks in the mixer | ONB-001, SCN-003, AUD-001, AUD-002, CAN-003 |
| 4 | Adjusts the camera position on the portrait canvas; adds noise suppression to the mic | Live preview updates; the filter is applied | SCN-007, AUD-007, CAN-007 |
| 5 | Adds a custom RTMP output (stream key) and enables recording + replay buffer (60 s) | Key stored in Credential Manager; pre-flight shows bandwidth and encoder usage | VID-004, SYS-006, VID-006, LIB-003, OUT-004 |
| 6 | Clicks "Go live" | Stream, recording and replay buffer start; health stats show | VID-007, VID-008 |
| 7 | During gameplay presses the marker hotkey twice and the save-clip hotkey once | Markers logged to the sidecar; clip saved within 2 s; toast confirms | ONB-005, LIB-005, LIB-004, LIB-002 |
| 8 | Stops the stream after 30 min | Outputs stop cleanly; recording remuxed to MP4; session record created; transcription job queued | VID-004, VID-006, LIB-002, AI-001, AI-007 |
| 9 | Opens the library, picks the saved clip, then "New clip project" | Editor opens with transcript, 16:9 and 9:16 variants; 9:16 uses the portrait-canvas footage | LIB-001, TL-001, TL-004, TL-006 |
| 10 | Trims the start and end on word boundaries; deletes a dead-air segment | Non-destructive cuts; preview plays the result | TL-002, TL-003, TL-008 |
| 11 | Enables captions on the 9:16 variant; fixes one misheard word | Caption track generated and edited | AI-002 |
| 12 | Queues both variants for export with presets | Background render; validation runs | EXP-001, EXP-002, EXP-004 |
| 13 | Reviews the result | Both files pass validation and play in a standard player | EXP-003 |

**Failure branches that must also pass**
- 6a: network drops for 20 s → stream reconnects (VID-005); recording and replay are unaffected.
- 7a: the camera is unplugged and replugged → placeholder, then automatic resume (ONB-003).
- 8a: the app is killed during recording → recovery on next launch, ≤ 2 s lost (SYS-002).
- 12a: the export is cancelled and retried → it completes (EXP-001).
- 13a: an artificially corrupted export → validation fails with a reason; publishing is blocked (EXP-003).

---

## UJ-01 First launch and device setup (P-B)
1. Wizard → probe → canvas choice → camera/mic → template → recording folder (ONB-001, ONB-002).
2. Optional encoder/bandwidth recommendation (ONB-006, P1).
3. Optional account connection: YouTube, Twitch chat read (ONB-007, P1).

Failure branches: camera blocked by Windows privacy settings → guided fix (SYS-005); no NVENC → software encoder with reduced recommended settings (VID-003, A-02).

## UJ-02 Build scenes and sources (P-A)
Create a collection → scenes → add display/window/camera/browser/image/text sources → arrange with transforms and snapping → group layers (P1) → set transitions → undo mistakes.
Requirements: SCN-001…SCN-012.
Failure branch: the browser source crashes → automatic restart; the scene still renders (SCN-003).

## UJ-03 Configure audio and overlays (P-A)
Add mic, desktop and (P1) per-app game audio → set levels with meters → monitor via headphones → add gain/noise suppression → set sync offset → route tracks to stream/recording/clips (P1) → add a browser-source alert overlay.
Requirements: AUD-001…AUD-011, SCN-003.
Failure branch: the USB mic disconnects → silence + badge, then automatic reattach (AUD-009).

## UJ-04 Start and stop a stream (P-B)
Add an output → pre-flight → go live → watch health → stop (clean FCUnpublish).
Requirements: VID-004, VID-005, VID-008, OUT-004.
Failure branches: wrong key → "auth rejected" error (VID-004); encoder overload → warning with a one-click fix (VID-009, P1).

## UJ-05 Stream to multiple destinations (P-A)
Add 3 outputs (for example Twitch landscape, YouTube landscape, a custom ingest in portrait) → the encoder graph shows sharing → start all → stop one mid-stream → others continue.
Requirements: OUT-001…OUT-005, CAN-005.
Failure branch: a fourth output is refused (OUT-001); insufficient encoder sessions are rejected in pre-flight (OUT-002).

## UJ-06 Record while streaming (P-C)
Recording at higher quality than the stream, separate encoder → multitrack audio (P1) → stop → remux.
Requirements: VID-006, VID-007, AUD-011.
Failure branch: disk nearly full → warning, then a clean stop (LIB-007).

## UJ-07 Create and edit highlights (P-A, P-C)
Entry points: saved replay clip (LIB-004), marker (LIB-005), Labs suggestion (AI-003), arbitrary range (TL-001) → trim/split/delete (TL-002) → transcript edit (TL-007, P1) → captions (AI-002).
Failure branch: no transcript yet → edit without word snapping; transcription is offered.

## UJ-08 Produce landscape and portrait outputs (P-A)
Live: dual canvas streams/records/buffers (CAN-001…CAN-007). Post: one clip project → 16:9 + 9:16 variants with independent framing and captions (TL-004, TL-005, TL-006); Labs auto-reframe (AI-005, P2).
Failure branch: no portrait footage for the range → preset crop of the landscape source with a notice (TL-006).

## UJ-09 Review, export and publish (P-C)
Queue variants → render → validation report → (gated, A-05) publish to YouTube with an optional AI-drafted title (AI-006) → status tracking → (P2) analytics feedback.
Requirements: EXP-001…EXP-007.
Failure branches: upload interrupted → resumes (EXP-005); quota exceeded → error with the API message, no silent retry.

## Journey-to-release mapping

| Journey | Release one | Notes |
|---|---|---|
| UJ-00 | Must pass | Release gate G1 in `acceptance-criteria.md` |
| UJ-01…UJ-06 | P0 paths must pass; P1 branches if shipped | |
| UJ-07, UJ-08 | P0 paths must pass; Labs paths best-effort | |
| UJ-09 | Export must pass; publishing only if gate G4 passes (A-05) | |
