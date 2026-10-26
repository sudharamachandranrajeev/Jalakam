# ADR 0001: Phase 0 product decisions

- Status: **Accepted**
- Date: 2026-10-26
- Approved by: project owner
- Context: [`docs/research/mvp-recommendation.md`](../research/mvp-recommendation.md) §5

| ID | Decision | Accepted option |
|---|---|---|
| D1 | Platform scope | Windows-only MVP: Windows 10 2004+ / 11, x64, NVIDIA-first, with a Media Foundation software fallback |
| D2 | Licence and media basis | Apache-2.0. FFmpeg as a dynamically linked LGPL build; NVENC/AMF/QSV hardware encoders; Media Foundation H.264 software fallback. No x264, no libobs, no GPL-only dependencies |
| D3 | Technology stack | Rust core + Tauri/React (TypeScript) UI. The core renders the preview into a native child surface. Spike the preview integration in M1 week 1 |
| D4 | AI location and cost | Local ASR and moment scoring; no stream media leaves the PC by default. Cloud LLM only for titles and descriptions: opt-in, with the user's own API key in the MVP |
| D5 | Multistream and publishing | Local multistream to at most 3 outputs, no cloud relay; publish to YouTube only. Start the TikTok Content Posting and Twitch Enhanced Broadcasting access applications in parallel |

## Consequences
- Every new dependency must be checked against D2 before it is added. Keep a third-party licence register in `docs/licences.md`, created when the first dependency is added.
- Build-model assignment follows `mvp-recommendation.md` §4. Opus 5.5 (high effort) owns the media spine and its review gate. Sonnet 5.5 (high effort) owns the application layer.
- The next step is M0: architecture and per-module ADRs.
