# MVP Scope: Release One

Release one is the first usable release of Jalakam. It must demonstrate the golden journey UJ-00 end to end: configure a scene, capture audio and video, record or stream, select a highlight, edit a clip, render it, and export a valid file. It must do this while meeting the streaming-stability targets.

Requirement IDs refer to `product-requirements.md`. Open assumptions are in `assumptions-and-decisions-needed.md`.

## 1. Must work before the first usable release (P0)

| Area | Requirements |
|---|---|
| Onboarding and devices | ONB-001, ONB-002, ONB-003, ONB-004, ONB-005 |
| Scenes and sources | SCN-001, SCN-002, SCN-003, SCN-005, SCN-007, SCN-008, SCN-011 |
| Audio | AUD-001, AUD-002, AUD-004, AUD-006, AUD-007, AUD-009, AUD-010 |
| Video pipeline | VID-001, VID-002, VID-003 (NVENC H.264 + MF fallback), VID-004, VID-005, VID-006, VID-007, VID-008, VID-010 |
| Multiple destinations | OUT-001, OUT-002, OUT-003, OUT-004 |
| Canvases | CAN-001, CAN-002, CAN-003, CAN-005, CAN-006, CAN-007 |
| Library and clipping | LIB-001, LIB-002, LIB-003, LIB-004, LIB-005, LIB-007 |
| AI (core, non-experimental) | AI-001 (post-recording transcription), AI-002 (captions), AI-007 (resource governor), AI-008 (model management) |
| Timeline | TL-001, TL-002, TL-003, TL-004 (16:9 + 9:16), TL-005 (presets + static crop), TL-006, TL-008, TL-010 |
| Export | EXP-001, EXP-002, EXP-003, EXP-004 |
| System | SYS-001, SYS-002, SYS-003 (local), SYS-004, SYS-005, SYS-006, SYS-007, SYS-008 (baseline), SYS-010, SYS-011 (installer), SYS-012 |
| Non-functional | NFR-PERF-01…04, NFR-REL-01…03, NFR-SEC-01/02, NFR-COMP-01, NFR-MNT-01 |

## 2. Important but deferrable (P1)

Planned for release one if milestones hold; otherwise moved to 1.1 without blocking release.

| Item | Requirements | Note |
|---|---|---|
| YouTube publishing + status | EXP-005, EXP-006, ONB-007 | **Gated:** enabled only after export gate G4 passes (A-05). The single validated publishing integration |
| AMD AMF / Intel QSV, NVENC HEVC/AV1 | VID-003 (P1 part) | A-18 |
| Media file source, groups, layer effects, studio mode | SCN-004, SCN-006, SCN-010, SCN-012 | |
| Per-app audio, LUFS, gate/compressor/limiter, routing + multitrack | AUD-003, AUD-005, AUD-008, AUD-011 | AUD-003 is OS-gated (A-19) |
| Overload assistant, encoder recommendation | VID-009, ONB-006 | |
| Add output while live, platform presets | OUT-005, OUT-006 | |
| Copy layer across canvases | CAN-004 | |
| Import external video | LIB-008 | |
| Keyframed reframe, transcript editing, editor audio, 1:1 variant | TL-005 (P1 part), TL-007, TL-009, TL-004 (1:1) | |
| Crash upload, full WCAG 2.2 AA, automation API, auto-update | SYS-003 (upload), SYS-008 (full), SYS-009, SYS-011 (update) | |
| Idle footprint, learnability | NFR-PERF-05, NFR-UX-01 | |

## 3. Experimental AI capabilities (Labs, off by default)

Shipped behind the Labs toggle; excluded from the release-one stability gates. They must still obey AI-007 (never harm the live output) and SYS-007 (privacy).

| Capability | Requirement | Pri | Graduation criterion to leave Labs |
|---|---|---|---|
| Post-session highlight suggestions with explanations and feedback capture | AI-003, AI-009, AI-010 | P1 | precision@10 ≥ 0.4 on the eval set (A-21) and ≥ 30 % of suggestions accepted in beta |
| AI-drafted title, description, hashtags (BYO key) | AI-006 | P1 | ≥ 50 % of drafts published with light edits in beta |
| Live transcription and live moment hints | AI-004 | P2 | No measurable increase in skipped/lagged frames in a 4 h soak (A-04) |
| Auto-reframe with subject tracking | AI-005 | P2 | ≥ 80 % of reframed seconds need no manual correction in UX tests |

## 4. Explicitly outside the initial scope

| Feature | Reason | Source |
|---|---|---|
| macOS and Linux | D1 | ADR 0001 |
| Twitch Enhanced Broadcasting (multitrack E-RTMP) | Access and eligibility unknown | OUT-007, Phase 0 U09 |
| Cloud multistream relay | D5 | ADR 0001 |
| More than 3 simultaneous outputs | D5 | ADR 0001 |
| Publishing to TikTok, Twitch, Instagram, X, Facebook | D5; the Phase 1 brief asks for one validated integration first | EXP-008 |
| Hook-based game capture | Anti-cheat risk; WGC covers capture | Phase 0 R07 |
| Virtual camera | Complexity vs MVP value | Phase 0 F29 |
| VST3 plugin hosting | Licence and scope | Phase 0 F09 |
| AI overlay/widget generator | Not core to the thesis | Phase 0 F30 |
| OBS scene-collection import | Deferred | Phase 0 F32 |
| Background removal/blur, stinger/move transitions, instant-replay overlay, ducking, editor overlays, analytics feedback | P2 | SCN-009, LIB-006, AUD-012, TL-011, EXP-007 |
| x264 / any GPL-only component | D2 | ADR 0001 |
| Localisation beyond English | A-15 | |

## 5. Release-one definition of done

Release one is done only when **all** of the following are true:

1. **Scope:** every P0 requirement in §1 is implemented and its acceptance criteria (`acceptance-criteria.md`) pass on the release candidate build.
2. **Golden journey:** UJ-00, including all five failure branches, passes on the reference machine and on the minimum-spec NVIDIA machine, executed by someone who did not write the code.
3. **Stability gates:** G1–G3 in `acceptance-criteria.md` pass. That means a 4 h soak meeting NFR-PERF-01, NFR-PERF-03 and NFR-REL-01, plus the fault-injection suite meeting NFR-REL-02 and NFR-REL-03.
4. **Export gate:** G4 passes: 100 consecutive exports across both presets validate, with zero invalid files reported as valid.
5. **Compatibility:** the NFR-COMP-01 matrix passes, including the software-encoder machine.
6. **Security and privacy:** the NFR-SEC-01 secret scan is clean; a network-capture test confirms only the SYS-007 allow-listed connections; the Opus review gate has signed off the credential, OAuth and network code.
7. **Licensing:** the CI licence scan passes; About → Licences is complete; the FFmpeg build configuration is published (SYS-012).
8. **Accessibility:** the SYS-008 baseline audit passes (keyboard-only golden journey, screen reader names on live controls, no colour-only status).
9. **Labs isolation:** with Labs on, gates G1 and G2 still pass. With Labs off, no Labs code path runs.
10. **Gated publishing:** either YouTube publishing passes G5 and is enabled, or it is disabled in the build and documented as 1.1.
11. **Operations:** a signed installer; crash dumps captured locally; diagnostics export works; config migration from the previous beta build is tested.
12. **Documentation:** user quick-start, known-issues list, and a privacy page listing every network destination.
13. **No open P0 defects**, and no open P1 defects in the golden journey.

## 6. Measurable success criteria

Measured on release-one builds. Lab metrics use the reference hardware (A-01). Beta metrics come from opt-in telemetry (A-13) or moderated tests.

| # | Metric | Target | Source |
|---|---|---|---|
| SC-01 | Compositor-lagged + encoder-skipped frames, 4 h soak (dual canvas, 2 outputs, recording, replay) | < 0.1 % | NFR-PERF-01 |
| SC-02 | A/V offset at start / after 4 h | ≤ 20 ms / ≤ 30 ms | NFR-PERF-03 |
| SC-03 | Crashes during the 4 h soak; crash-free beta sessions | 0; ≥ 99.5 % of sessions | NFR-REL-01, telemetry |
| SC-04 | Recording lost on forced kill | ≤ 2 s | NFR-REL-02 |
| SC-05 | Game FPS impact | ≤ 10 % | NFR-PERF-02 |
| SC-06 | Clip save latency (hotkey to file on disk) | ≤ 2 s at p95 | LIB-004 |
| SC-07 | Export validity: valid-file rate across the G4 run; false "valid" verdicts | 100 %; 0 | EXP-003 |
| SC-08 | Render speed, 60 s 1080p clip (hardware path) | ≤ 30 s | EXP-004 |
| SC-09 | Time from opening a saved clip to an exported, validated 9:16 captioned file (moderated test, median) | ≤ 2 min | UJ-00 steps 9–13 |
| SC-10 | First-time streamers completing UJ-00 unaided in ≤ 15 min | ≥ 4 of 5 | NFR-UX-01 |
| SC-11 | Transcription throughput on reference GPU | ≥ 4× real time | AI-001 |
| SC-12 | Caption word accuracy on clean English speech (eval set) | WER ≤ 12 % | AI-002 |
| SC-13 | Labs highlight precision@10 (eval set) | ≥ 0.4 | AI-003, A-21 |
| SC-14 | Cold start to interactive | ≤ 5 s | NFR-PERF-04 |
| SC-15 | Secrets found by the release secret scan | 0 | NFR-SEC-01 |
| SC-16 | Beta retention: weekly active streamers who saved ≥ 1 clip in week 1 and are still streaming with Jalakam in week 4 | ≥ 40 % | Telemetry (opt-in) |
