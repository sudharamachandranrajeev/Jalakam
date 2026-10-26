# ADR-0010: Local-first AI; optional cloud LLM for metadata only

- **Status:** Accepted
- **Related:** ADR 0001 D4, PRD AI-*, assumptions A-04, A-06

## Context
AI features in scope:
- transcription;
- highlight scoring;
- visual detection (Labs);
- title and description suggestions.

Constraints:
- Users stream private content.
- Cloud costs must not fall on the project.
- The app must work offline.

## Decision
- **ASR:** whisper.cpp (MIT) in the AI worker.
  - Models download on demand, with pinned SHA-256 checksums.
  - It uses the GPU (CUDA or Vulkan backend) only when idle, and the CPU while live.
  - Release one transcribes **after** a recording. Live ASR is Labs (A-04, pending confirmation).
- **VAD:** Silero VAD (MIT) via ONNX Runtime (MIT), on the CPU.
- **Scoring:** local heuristics in release one: audio energy, speech rate and markers; no chat signals. Labs adds a small local model.
- **Visual detection (Labs):** ONNX Runtime with DirectML, idle only.
- **LLM:**
  - optional, and opt-in per feature;
  - uses the user's own API key;
  - text in, text out, with no tools;
  - behind a provider adapter trait; the default adapter is the Anthropic API;
  - never sends audio or video.

## Rejected
- **Cloud ASR:** fails the privacy, cost and offline requirements (D4).
- **A Python runtime for inference:** worse packaging size, startup time and sandboxing than native libraries.
- **Bundling models in the installer:** installer size; a medium Whisper model is ~1.5 GB.

## Consequences
- Quality depends on local hardware.
- CPU-only machines get smaller models and slower transcription (performance-budgets.md §8).
- The model registry, download resume and checksum code are new work.

## Validation
POC-1B runs whisper.cpp in a capped worker during a live POC-1A session. It measures the effect on dropped frames and audio glitches. Target: no change beyond noise.
