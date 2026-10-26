# ADR-0005: Controlled multi-process architecture

- **Status:** Accepted
- **Related:** technology-evaluation.md E4/E6, system-design.md §3–§5, NFR-REL-03

## Context
None of these may end a live stream: a crash or stall in the UI, a browser overlay, a media parser or an AI model. AI and render jobs must be bounded in CPU, memory and GPU use.

## Options
1. **Single process** with threads.
2. **Controlled multi-process:** App Host, Media Engine, CEF browser, workers (ai | render) and a crash handler.
3. **Fine-grained:** a process per source or per output. More isolation, but much more IPC and GPU-sharing overhead.

## Decision
Option 2.

| Process | Role | Job Object / priority |
|---|---|---|
| `jalakam.exe` (App Host) | UI, state, persistence, jobs, governor, publishing, secrets, supervisor | Normal |
| `jalakam-engine.exe` | Real-time capture, compositing, encoding, outputs | `ABOVE_NORMAL`. Not killed with the App Host; 10-minute orphan grace period |
| `jalakam-browser.exe` | CEF off-screen rendering, plus CEF subprocesses | In the engine's job; memory-capped |
| `jalakam-worker.exe --role=ai\|render` | Inference, probe, decode, export | Separate job: CPU-rate cap, memory cap, kill-on-close, `IDLE`/`BELOW_NORMAL` |
| `jalakam-crash.exe` | Crashpad handler | Normal |

The engine survives an App Host crash:
1. The watchdog relaunches the App Host.
2. The new App Host reattaches over the pipe.
3. It reads the engine's desired-state revision (ADR-0009).

If no host reattaches within 10 minutes, the engine stops its outputs cleanly and exits.

## Consequences
- Crash containment matches the failure domains in NFR-REL-03.
- Job Objects give hard CPU-rate and memory limits for background work (ADR-0011).
- Costs:
  - IPC design (ADR-0008);
  - cross-process GPU sharing for the preview and browser sources (shared NT handles);
  - version negotiation;
  - a supervisor.
- Debugging needs multi-process tooling. Every process logs with a shared `trace_id`. A Visual Studio "Child Process Debugging Power Tool" profile is documented.

## Rejected
- **Single process:** an AI or parser crash ends the stream, and GPU and memory cannot be capped per subsystem.
- **Process per source or output:** each extra GPU-sharing boundary adds latency and synchronisation, and release one does not need it. CEF is already isolated as its own process tree.

## Validation
Fault-injection tests FI-01..FI-12 (acceptance-criteria.md). Each test kills one process while live and checks that the stream continues or degrades as specified.
