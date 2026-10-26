# Acceptance Criteria

This document turns the requirements in `product-requirements.md` into testable release gates and scenario tests. Each scenario names the requirements it proves. A requirement is accepted when its inline acceptance criteria pass *and* every scenario that cites it passes.

## 1. Test environments

| ID | Machine | Purpose |
|---|---|---|
| ENV-REF | Reference hardware (A-01): Windows 11, RTX 3060 12 GB, 6-core CPU, 16 GB RAM, NVMe | All performance gates |
| ENV-MIN | Minimum NVIDIA (A-02): GTX 1660-class, 4-core CPU, 8 GB RAM, Windows 10 22H2 | Compatibility, reduced targets (720p60 single canvas) |
| ENV-SW | No NVENC (integrated GPU or AMD with AMF disabled) | Software-fallback paths |
| ENV-NET | Network shaper between the test PC and a local RTMP server (for example a MediaMTX or nginx-rtmp container) | Reconnect and bandwidth tests |

Test ingest: a local RTMP/RTMPS server that records what it receives, so stream-side measurements need no live platform. One real-platform smoke test (YouTube unlisted) runs per release candidate.

## 2. Measurement methods

| Method | Definition |
|---|---|
| **Frame accounting** | The app's own counters (VID-008) cross-checked against frame counts in the recorded file and at the ingest. *Lagged* = compositor missed its deadline; *skipped* = encoder dropped input; *network-dropped* = frames discarded by the output queue. NFR-PERF-01 counts lagged + skipped only |
| **Clapper test** | A scene contains a source that flashes a white frame and emits a 1 kHz beep at the same clock tick every 10 s. An analysis script measures the offset between flash and beep onset in the recording and the ingest capture. Report median and max per 10-minute window |
| **Valid file** (EXP-003, G4) | (1) the probe reads the container without errors; (2) a full decode of every stream to null output reports zero errors; (3) codec, profile, pixel format, resolution and fps match the preset; (4) duration matches the edit decision list within ±1 frame; (5) A/V start offset ≤ 1 audio frame (≈ 21 ms at 48 kHz/1024); (6) preset platform limits pass; (7) the file plays in the Windows "Media Player" app and in Chromium |
| **Game impact** | A fixed-path GPU-bound benchmark scene (any game with a built-in benchmark), 3 runs with and 3 runs without Jalakam live; compare mean FPS |
| **Eval set** | ≥ 20 h of the team's own VODs with human-labelled highlight windows (A-21) and ≥ 2 h with human transcripts for WER |

## 3. Release gates

| Gate | Name | Pass condition | Requirements |
|---|---|---|---|
| **G1** | Golden journey | UJ-00 including failure branches 6a, 7a, 8a, 12a and 13a passes on ENV-REF and ENV-MIN, executed by a tester who did not write the code; repeated on 3 separate days | All P0 in the journey |
| **G2** | 4-hour soak | ENV-REF, 1080p60 landscape + portrait canvases, 2 RTMP outputs (one per canvas), recording (landscape), replay buffer (both canvases), one browser source, camera, mic, desktop audio. Over 4 h: lagged + skipped < 0.1 %; A/V ≤ 20 ms at start and ≤ 30 ms at end; 0 crashes; 0 output stalls > 1 s; RSS growth ≤ 5 % after minute 15. A 30-clip save run during the soak (one every 8 min) adds no drop spikes > 0.1 % within a 10 s window | NFR-PERF-01/03, NFR-REL-01, LIB-003/004 |
| **G3** | Fault injection | All FI scenarios in §4.2 pass | NFR-REL-02/03, SYS-002, ONB-003, AUD-009, VID-005, LIB-007 |
| **G4** | Export reliability | 100 consecutive exports (50 × 16:9, 50 × 9:16 with burned captions; sources varied: replay clips, recordings, imported files) all meet "valid file"; a planted set of 10 corrupted outputs is 100 % detected as invalid | EXP-001…004, AI-002, TL-004/006 |
| **G5** | YouTube publishing (gated, A-05) | Only after G4. 20 uploads (mixed 16:9/9:16) succeed to a test channel; 3 forced network interruptions resume without re-uploading confirmed chunks; revoked token produces a re-connect prompt; no publish happens without explicit confirmation | EXP-005/006, ONB-007, SYS-006 |
| **G6** | Security, privacy, licensing | Secret scan clean (NFR-SEC-01); network capture over UJ-00 shows only allow-listed destinations (SYS-007); no listening sockets by default (NFR-SEC-02); licence scan passes (SYS-012) | SYS-006/007/012, NFR-SEC-01/02, NFR-MNT-01 |
| **G7** | Accessibility baseline | UJ-00 steps 5–8 (live controls) completed keyboard-only; NVDA reads accessible names for all live controls; automated contrast check passes on the main screens; no status conveyed by colour alone | SYS-008, CAN-003 |
| **G8** | Compatibility | G1 core path on ENV-SW (software encoder at 720p30) and on Windows 10 22H2 + Windows 11 latest | NFR-COMP-01, VID-003 |

## 4. Scenario tests (Given / When / Then)

### 4.1 Core functional scenarios

**AC-ONB-01 First-run wizard produces a working scene** (ONB-001, ONB-002, CAN-002)
- Given a clean user profile with a webcam and microphone attached,
- When the user completes the wizard choosing "Landscape + Portrait", the webcam, the mic and the default template,
- Then a scene exists with display and camera layers on both canvases, the mixer shows mic and desktop tracks with moving meters, and relaunching the app does not show the wizard again.

**AC-ONB-02 Hot-plug camera** (ONB-003)
- Given the app is live with a camera layer,
- When the camera is unplugged for 10 s and replugged,
- Then within 1 s of unplug the layer shows a "disconnected" placeholder, the stream continues without stalls, and within 3 s of replug the live camera image returns without user action.

**AC-SCN-01 Transform and undo** (SCN-007, SCN-011)
- Given a scene with a camera layer,
- When the user moves, scales, rotates and crops the layer, then presses Ctrl+Z four times,
- Then the layer returns exactly to its original transform, and Ctrl+Y re-applies each step in order.

**AC-AUD-01 Device loss does not stall the mix** (AUD-009, AUD-002)
- Given a USB microphone track and desktop audio are being recorded,
- When the USB mic is unplugged for 15 s and replugged,
- Then the recording contains continuous desktop audio with no gap > 10 ms, the mic track is silent while unplugged, and mic audio resumes within 3 s of replug.

**AC-VID-01 Independent record and stream** (VID-006, VID-007, OUT-002)
- Given a stream output at 1080p60 6 Mbps and a recording at 1080p60 CQ-quality,
- When both run for 10 minutes,
- Then the encoder graph shows two encoder sessions, the ingest receives ~6 Mbps, the recording bitrate differs from the stream's, and both files decode without errors.

**AC-VID-02 Reconnect** (VID-005)
- Given a live output on ENV-NET,
- When the network is blocked for 20 s and then restored,
- Then the output enters "Reconnecting", attempts follow exponential backoff, the stream resumes within 5 s of restoration, and the local recording has no gap.

**AC-VID-03 Clean stop** (VID-004)
- Given a live RTMP output to the test server,
- When the user stops the output,
- Then the server log shows FCUnpublish before the TCP close.

**AC-OUT-01 Three outputs, one stops** (OUT-001, OUT-003)
- Given three outputs live (two sharing one encoder, one on the portrait canvas),
- When output 2 is stopped,
- Then outputs 1 and 3 keep sending with no increase in skipped frames, and adding a fourth output is refused with an explanation.

**AC-OUT-02 Pre-flight blocks invalid configuration** (OUT-004, VID-004)
- Given an output whose URL has no scheme,
- When the user clicks "Go live",
- Then pre-flight reports an error naming the output and the problem, and nothing starts.

**AC-CAN-01 Canvas lock while live** (CAN-006)
- Given any output, recording or replay buffer is active,
- When the user opens canvas preferences,
- Then enable/disable portrait, resolution and fps controls are disabled with an explanation of what to stop.

**AC-LIB-01 Clip save** (LIB-003, LIB-004)
- Given the replay buffer has run for ≥ 60 s at 1080p60,
- When the save-clip hotkey is pressed while a full-screen game has focus,
- Then within 2 s an MP4 of 60 s (±1 GOP) exists in the clips folder, starts on a keyframe, passes "valid file" checks 1–3 and 7, and the toast offers "Open in editor".

**AC-LIB-02 Markers reach the editor** (LIB-005, LIB-002, TL-001)
- Given a recording during which three markers were added,
- When the recording is opened in the editor,
- Then three markers appear at their timestamps (±1 frame) and "New clip project" from a marker creates a range of marker −30 s to +15 s.

**AC-AI-01 Transcription while idle only uses spare capacity** (AI-001, AI-007)
- Given a 30-minute recording has just ended and a new stream starts immediately,
- When the transcription job is queued,
- Then while live the job runs at low priority or pauses on health warnings, G2 frame targets still hold, and after going offline the transcript completes with word timestamps.

**AC-AI-02 Captions** (AI-002)
- Given a clip with a transcript,
- When captions are enabled on the 9:16 variant and one word is corrected,
- Then the rendered file shows the corrected word, lines are ≤ 42 characters, at most 2 lines, and the SRT sidecar matches the burned-in text and timing (±100 ms).

**AC-TL-01 Non-destructive edits** (TL-002, TL-003)
- Given a clip project from a recording,
- When the user trims, splits and deletes segments, then closes and reopens the project,
- Then all edits persist, the source file's hash is unchanged, and undo history is available.

**AC-TL-02 Independent variants** (TL-004, TL-005, TL-006)
- Given a project with portrait-canvas footage for the same range,
- When the 9:16 variant uses the portrait source and the 16:9 variant uses a "fill" preset with a different extra trim,
- Then each export reflects only its own framing and trim, and the shared cuts are aligned within 1 frame.

**AC-EXP-01 Queue survives restart** (EXP-001)
- Given three exports are queued and one is rendering,
- When the app is killed and relaunched,
- Then the completed export stays completed, the interrupted one restarts from the beginning, and the third remains queued.

**AC-EXP-02 Invalid output is blocked** (EXP-003)
- Given an export whose output file is truncated by the test harness after render,
- When validation runs,
- Then the item is marked "invalid" with the failing check named, and publishing is unavailable for it.

**AC-SYS-01 Secrets never leak** (SYS-006, NFR-SEC-01)
- Given a stream key and a YouTube token are configured,
- When the user runs UJ-00, forces a crash, and exports diagnostics,
- Then a scan of app data, logs, the crash dump and the diagnostics zip finds neither secret.

**AC-SYS-02 Privacy allow-list** (SYS-007)
- Given a fresh install with telemetry and Labs off,
- When UJ-00 is executed under a network capture,
- Then the only outbound connections are the configured ingest, the update check and the model download.

### 4.2 Fault-injection scenarios (gate G3)

| ID | Fault | Expected result | Requirements |
|---|---|---|---|
| FI-01 | `taskkill /F` during recording (MKV) | On relaunch, recovery offered; recovered file plays; ≤ 2 s lost | SYS-002, NFR-REL-02 |
| FI-02 | Same with the fragmented-MP4 option | Same | SYS-002, A-10 |
| FI-03 | Browser-source renderer process killed | Source restarts within 3 s; outputs unaffected | SCN-003, NFR-REL-03 |
| FI-04 | Encoder session forced to fail (driver reset simulated by killing the NVENC session) | Affected output stops with an error; recording on another session continues; fallback offered | VID-003, NFR-REL-03 |
| FI-05 | Disk filled to < 1 GB during recording | Recording stops cleanly, file finalised and valid, alert shown | LIB-007, VID-006 |
| FI-06 | Network blocked 20 s, then 20 min | 20 s: reconnect; 20 min: backoff continues, user can cancel; recording unaffected | VID-005 |
| FI-07 | USB mic unplugged and replugged | AC-AUD-01 | AUD-009 |
| FI-08 | Camera unplugged and replugged | AC-ONB-02 | ONB-003 |
| FI-09 | Corrupt prefs and collection files | Defaults / recovery prompt; bad files backed up | ONB-004, SCN-001, SYS-010 |
| FI-10 | Crash during export | Queue resumes per AC-EXP-01 | EXP-001 |
| FI-11 | Model file corrupted | Checksum mismatch detected, re-download offered | AI-008 |
| FI-12 | Migration failure (planted invalid schema) | Snapshot restored; previous state opened read-only | SYS-010 |

## 5. Traceability

Every P0 requirement in `mvp-scope.md` §1 must appear in at least one gate or scenario above, or have its inline acceptance criteria verified by the method in its Verify column. CI maintains the requirement → test mapping. A release candidate is blocked if any P0 requirement has no passing test linked to it.
