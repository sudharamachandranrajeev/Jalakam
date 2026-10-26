# ADR-0007: Capture APIs: WGC, Media Foundation, WASAPI and CEF off-screen rendering

- **Status:** Accepted
- **Related:** PRD SCN-*, AUD-*, unknowns-and-risks.md

## Context
Release one captures:
- displays, windows and games;
- webcams and capture cards;
- microphones, desktop audio and per-application audio;
- web overlays.

## Decision

| Source | API | Notes and limits |
|---|---|---|
| Display, window, game (windowed/borderless) | **Windows.Graphics.Capture** (WGC) | Windows 10 shows a yellow border. Borderless capture (`IsBorderRequired=false`) needs Windows 11 / build 20348+. Exclusive-fullscreen games may need borderless mode (documented in help) |
| Fallback display capture | **DXGI Desktop Duplication** | Used only if WGC fails, e.g. old drivers or some RDP sessions |
| Webcam, capture card | **Media Foundation** source reader with a DXGI device manager | MJPEG/NV12/YUY2. MJPEG uses hardware decode where available |
| Mic, desktop audio | **WASAPI** shared mode, event-driven, 10 ms period; thread under MMCSS "Pro Audio" | Release one does not offer exclusive mode |
| Per-app audio | **WASAPI process loopback** (`ActivateAudioInterfaceAsync` with `AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK`) | Needs Windows 10 build 20348+ or Windows 11; hidden on older builds |
| Browser source | **CEF** off-screen rendering with `shared_texture_enabled` (accelerated paint) | CEF ships without proprietary codecs, so H.264 in overlays needs a custom CEF build. Release one documents the limit |

All sources stamp frames with QPC time. The engine clock is QPC. The render loop waits on a high-resolution waitable timer (`CREATE_WAITABLE_TIMER_HIGH_RESOLUTION`).

## Rejected
- **DirectShow for cameras:** legacy; MF covers modern devices. DirectShow-only capture cards are a known gap, listed in unknowns-and-risks.md.
- **Game hooking / DLL injection (OBS-style game capture):** conflicts with anti-cheat, costs a lot to maintain, and is a security posture we do not want. WGC covers windowed and borderless games.
- **WebView2 for browser sources:** it has no supported off-screen shared-texture output.

## Consequences
Exclusive-fullscreen capture is weaker than in hook-based tools. Mitigation: onboarding detects exclusive fullscreen and advises borderless mode.

## Validation
- POC-1A covers WGC display, MF camera, and WASAPI mic and loopback.
- POC-2 (optional) covers CEF shared textures.
