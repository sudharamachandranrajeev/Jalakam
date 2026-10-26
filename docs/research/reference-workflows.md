# Reference Workflows

How the reference product handles each user journey, followed by the target behaviour for Jalakam. Labels: **[O]** observation (cited), **[H]** hypothesis, **[R]** recommendation, **[U]** unknown (not documented and not observed in this pass).

The UI was not driven manually in this pass. No screenshots or recordings were taken. Steps come from public docs and static packaging inspection. A manual UI walkthrough is the next non-invasive step; see `unknowns-and-risks.md`.

---

## J1. First launch and device setup
- [O] Windows install comes from an MSIX/`.appinstaller` package that auto-updates on launch. macOS uses a DMG. (S11, L01)
- [O] On first launch, a dialog offers a standard horizontal canvas or Multi Canvas. This can be changed later in preferences. (S04, search summary of S14)
- [O] An onboarding flow is shown once, and its completion is persisted. Scene templates are offered. (S01, L04)
- [O] OBS scene collections can be imported. (L04)
- [U] Device permission prompts, the camera/mic wizard order, and the account-sign-in requirement are unknown.
- [R] Jalakam: hardware probe (GPU, encoders, displays, audio devices) → canvas choice (landscape / landscape+portrait) → pick camera and mic → choose a template → optional platform sign-in. Offer OBS import.

## J2. Creating scenes and adding sources
- [O] A scene holds an ordered layer stack. Layer types include display capture, window/game capture, camera, media file (local only; GIFs load as images), image, browser source (CEF), text, widgets and duplicators. (S01, S04, L04)
- [O] With Multi Canvas on, each scene has a separate layer stack per canvas. Main-canvas selection is outlined in blue and portrait in purple. Layers can be copied across canvases. (S04)
- [O] Each layer supports effects: chroma key, color grading, blurs, shadows, background removal. (S01, S12)
- [O] Scenes can be staged and then shown (preview/program pattern). (S10)
- [R] Jalakam: same scene → canvas → layer model. Every scene object has a stable ID, so the automation API can address it.

## J3. Configuring audio and overlays
- [O] Audio tracks come from input devices, output devices (Windows) or processes. They are global by default; unlinking makes volume and mute per-scene. Layer-bound tracks follow their scene. (S05)
- [O] Per-track settings: mute, mono/stereo, channel remap and delay. Effects: Voice Clarity, gain, compressor with sidechain ducking, panner, and VST3 (experimental). CUE monitoring. A VOD-exclude flag for Twitch. (S05)
- [O] Overlays are browser-source widgets, Spark-generated HTML/JS, or third-party alert tools. (S01, S07)
- [R] Jalakam: same model. Add a loudness meter (LUFS) and a per-track "include in clips" flag for AI scoring and clip audio.

## J4. Starting and stopping a stream
- [O] Outputs are configured per platform. Native outputs use account sign-in; custom outputs take a URL and key. A URL without a scheme is rejected. (S03, L04)
- [O] A go-live review dialog exists. Outputs can be started together or one at a time. A test recording/review flow exists. (S03, L04, L05)
- [O] With Twitch Enhanced Broadcasting, the app fetches its encoder configuration from Twitch when the stream starts. (S15, S17, L04)
- [R] Jalakam: pre-flight check (bandwidth estimate vs summed bitrates, encoder sessions available) → go-live review → start. Reconnect with exponential backoff and jitter, at most 25 attempts. Send FCUnpublish on a clean stop. (S17)

## J5. Streaming to multiple destinations
- [O] Locally: each output sends from the PC; encoders are shared when the canvas and settings match; bitrates add up. (S03)
- [O] In the cloud: one RTMP ingest to Meld Multi, which fans out server-side. (S08)
- [O] Twitch EB constrains which canvases other outputs can use. (S03) Third-party sources say EB is incompatible with multistreaming in some other apps. (S20)
- [R] Jalakam MVP: local multistream with encoder sharing. Cloud relay is a later decision (decision D5).

## J6. Recording while streaming
- [O] Supported. The recording encoder is independent, so you can record 4K while streaming 1080p. Under Twitch EB, recording follows a Twitch track and you must start streaming first. (S06)
- [O] Formats: MP4, MOV, MKV, FLV. Multitrack audio is supported; MKV is recommended. (S06)
- [R] Jalakam: default to fragmented MP4 or MKV so a crash does not lose the recording. Write a sidecar event log (markers, chat spikes, transcript) next to each recording to feed the AI editor.

## J7. Creating and editing highlights
- [O] Clipping keeps an encoder running even when not live. The Clip button saves the last N seconds (10–120). The API command `meld.recordClip` takes a duration of 0–90 s. (S02, S10)
- [O] Replay Clip offers to play the clip back in-scene for 10 s, with scrub, A/B loop, speed control and an audio toggle. (S02)
- [O] Meld documents no trimming, captioning or AI selection. (S02)
- [R] Jalakam: (a) a manual clip hotkey or button, exactly like the reference; (b) AI markers suggested live in a side panel; (c) a post-stream editor with a transcript timeline, trim, captions and reframing.

## J8. Producing landscape and portrait outputs
- [O] The Portrait canvas is a second compositor output at 1080×1920 with its own bitrate. Streaming, recording, clipping and the virtual camera can each target either canvas. (S04, S02, S06)
- [O] Multi Canvas cannot be toggled while any output is running. (S04)
- [R] Jalakam: same. Add an "AI reframe" path for clips that only exist in landscape.

## J9. Reviewing, exporting and publishing clips
- [O] Clips are saved to a local folder in the chosen container. There is a share-to-X intent. (S02, L04)
- [U] No documented review queue or direct platform upload was found.
- [R] Jalakam: a review queue (accept, edit, reject) → export presets (9:16 Shorts/TikTok, 16:9 YouTube, 1:1) → publish through platform APIs with AI-drafted title, description and hashtags. A human must confirm each publish.
