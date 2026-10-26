# ADR-0014: Encoder graph, bounded output queues and reconnect state machine

- **Status:** Accepted
- **Related:** PRD OUT-*, LIB-003/004, system-design.md output state machine

## Context
Up to 3 RTMP outputs, a recording and the replay rings must:
- share encoders where settings match;
- survive network drops;
- never let one slow consumer stall the others.

## Decision
- **Encoder graph:**
  - Encoder sessions are keyed by (canvas, codec, resolution, fps, rate control, bitrate, keyframe interval).
  - Outputs with identical keys share a session.
  - The graph checks the GPU's NVENC session limit before configuring.
- **Broadcast:** each encoded packet goes to bounded queues, one per consumer, sized by duration. The default for network outputs is 2 s.
- **Drop policy** when a queue is full:
  1. Drop non-reference frames first.
  2. If still full, drop until the next IDR.

  The encoder is never blocked. Audio stays in sync by dropping the matching audio spans.
- **Replay ring:** encoded packets indexed by IDR. Saving muxes a window that starts at an IDR into MP4, on a background thread.
- **Output state machine:**
  - States: `Idle → Connecting → Live → Reconnecting → Live | Failed`.
  - Exponential backoff with jitter: starts at 2 s, capped at 15 min, at most 25 attempts.
  - Auth errors go straight to `Failed` with `OUT.RTMP.AUTH_REJECTED`.
  - A clean stop sends `FCUnpublish` + `deleteStream`.
- **Recording:**
  - Independent of network outputs.
  - Disk full stops it cleanly and finalises the file.
  - Records MKV, with remux to MP4 on request.
- **Error codes:** stable and namespaced (`OUT.*`, `ENC.*`, `CAP.*`, `REC.*`), mapped to UI messages and help links.

## Rejected
- **Always one encoder per output:** wastes NVENC sessions and GPU time.
- **Blocking back-pressure:** a slow network would stall recording.
- **Infinite retry:** hides configuration problems and risks platform rate limits.

## Validation
- POC-1A: pull the network cable or block the port for 30 s. Check reconnect, recording continuity and replay save.
- The FI tests in acceptance-criteria.md.
