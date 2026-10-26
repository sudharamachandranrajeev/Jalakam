# ADR-0004: Custom real-time pipeline with FFmpeg libraries (LGPL) for encode, mux and decode

- **Status:** Accepted
- **Related:** ADR 0001 D2, technology-evaluation.md E3, ADR-0006, ADR-0014

## Context
Release one needs:
- capture and compositing of two canvases;
- encoder sharing;
- three outputs, recording and a replay buffer;
- offline export.

Licence constraint: Apache-2.0 with no GPL dependencies.

## Options
1. **Custom pipeline + FFmpeg libraries.** We write our own clock, render graph, audio mixer and encoder graph. FFmpeg (libavcodec, libavformat, libswresample; LGPL build, dynamically linked) supplies encoders, muxers, RTMP/FLV and offline decode.
2. **GStreamer:** LGPL core; plugins carry mixed licences.
3. **libobs (the OBS core):** excluded, because it is GPL-2.0.
4. **Vendor SDKs only** (NVENC SDK, MF) with our own muxers: too much protocol and container work.

## Decision
Option 1. We own the real-time graph: capture, timing, compositing, mixing and queues.

FFmpeg provides:
- encoders: `h264_nvenc` with D3D11 hwframes, `h264_amf`, `h264_qsv`, `h264_mf`;
- the AAC path; the MF AAC encoder is preferred (see threat-model.md and the licensing note in unknowns-and-risks.md);
- FLV/RTMP, Matroska and MP4 muxers;
- decoders for the editor and the render worker.

FFmpeg is built with `--disable-gpl --disable-nonfree --enable-shared`. The configure line is published with each release.

## Consequences
- **Control:** we decide timing, drop policy, encoder sharing and recovery. These are the parts that decide stream quality.
- **Ownership cost:** we own the hardest real-time code. That is why the media spine is Opus-owned and POC-first.
- **RTMP limits:** FFmpeg's RTMP client is basic (no enhanced RTMP, limited reconnect hooks). We wrap it in our own state machine (ADR-0014) and may replace it with an in-house RTMP client later.
- **LGPL compliance:** dynamic linking, an offer of the FFmpeg source, and relinking allowed.

## Rejected
- **GStreamer (E3: 37 vs 45):** technically viable, but it loses on three points:
  - Dynamic reconfiguration under live load. Adding and removing outputs and sources without glitches needs expert-level pad blocking.
  - Windows packaging size.
  - The plugin-licence curation needed to stay GPL-free.
- **libobs:** GPL-2.0 conflicts with D2.
- **Vendor SDKs only:** we would reimplement FLV/RTMP/MKV/MP4 for no gain.

## Validation
POC-1A must show:
- zero-copy NVENC from a D3D11 texture through FFmpeg hwframes;
- RTMP to a local MediaMTX server;
- MKV recording;
- replay save;
- a 1 h soak with < 0.1 % dropped frames.
