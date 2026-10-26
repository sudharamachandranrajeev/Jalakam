# ADR-0008: IPC: named pipes with JSON-RPC for control; shared memory and shared textures for data

- **Status:** Accepted
- **Related:** ADR-0005, threat-model.md T-03..T-05, NFR-SEC-02

## Context
Control messages are small and infrequent. Meters, stats and preview frames are high-rate. Security rules out network listeners and access from other local processes.

## Decision
- **Control:**
  - One duplex named pipe per child process, with overlapped IO.
  - Messages are length-prefixed **JSON-RPC 2.0**, with types from `jk-protocol`.
  - The protocol version is negotiated at connect.
  - Request IDs make mutating calls idempotent.
  - Every call has a deadline.
- **High-rate telemetry:** a shared-memory section (`CreateFileMapping`, user-only DACL). A seqlock ring carries meters (30 Hz) and stats (1 Hz).
- **Preview frames:**
  - A ring of 3 D3D11 textures, created with `D3D11_RESOURCE_MISC_SHARED_NTHANDLE | SHARED_KEYEDMUTEX`.
  - The NT handles are duplicated into the App Host on attach.
  - The engine never waits for the host. If it cannot acquire a keyed mutex with a 0 ms timeout, it skips that slot.
- **Security:** see threat-model.md T-03/T-04 (DACL, first instance, peer PID check, per-launch token).
- **Future encoding:** if profiling shows a need, a binary encoding (MessagePack or bincode) can replace JSON behind the same message types.

## Rejected
- **Localhost HTTP/WebSocket/gRPC:** violates NFR-SEC-02 by exposing local web attack surface such as DNS rebinding. It also adds port management.
- **COM out-of-process servers:** registration and marshaling complexity.
- **Copying preview frames through pipes or shared CPU memory:** 1080p60 BGRA is ~500 MB/s, and the path adds GPU→CPU→GPU copies.

## Consequences
JSON is easy to debug, because logs show the messages. The cost is a few hundred µs per message. That fits the control-traffic budget (p99 ≤ 10 ms).

## Validation
- POC-1C uses the shared preview ring.
- An IPC benchmark checks p99 latency under the reference load.
