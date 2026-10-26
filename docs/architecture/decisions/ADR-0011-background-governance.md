# ADR-0011: Background work governance: governor, Job Objects and GPU priority

- **Status:** Accepted
- **Related:** performance-budgets.md §3–§5, system-design.md (governor), NFR-PERF-02

## Context
AI and render jobs must never starve live capture and encoding.

Windows gives hard CPU and memory limits through Job Objects. It has **no hard GPU share limit**: GPU control is limited to priority and cooperation.

## Decision
A **governor** in the App Host reads engine health at 1 Hz:
- lagged and skipped frames;
- encoder queue depth;
- GPU frame time;
- audio underruns.

From these it sets one of four states:

| State | Condition | Background policy |
|---|---|---|
| Idle | No output or recording is live | CPU ≤ cores − 1, `BELOW_NORMAL`. GPU allowed at GPU thread priority −7, if the VRAM budget check (`QueryVideoMemoryInfo`) passes |
| Live-Green | Live and healthy | CPU-rate cap ≤ 2 cores, `IDLE` priority, background IO priority, **no GPU** |
| Live-Amber | Any of: lag > 0.05 % over 10 s; queue depth rising; GPU p95 > 4 ms | Pause jobs at the next checkpoint |
| Live-Red | Lag > 0.5 % or an audio underrun | Pause, plus a hard CPU-rate cap of 1 %, until the state has been Green for 30 s |

Mechanisms:
- **Job Object limits:** `JOB_OBJECT_CPU_RATE_CONTROL_HARD_CAP` and `JOB_OBJECT_LIMIT_JOB_MEMORY`.
- **Budget messages:** the governor sends `budget.update` to the workers.
- **Cooperative checks:** workers check a flag between chunks; one chunk is ≤ 500 ms of work.
- **GPU priority:** the engine raises its own GPU thread priority (`IDXGIDevice::SetGPUThreadPriority(+7)`); workers lower theirs.
- **Hysteresis:** state changes use hysteresis to prevent flapping.

## Rejected
- **Thread-pool priority only:** not enough under memory pressure, and there is no hard cap.
- **GPU partitioning (MIG, vGPU):** not available on consumer GPUs.
- **Running AI in the engine:** violates crash containment (ADR-0005).

## Consequences
In release one, GPU-heavy AI waits for idle time. On GPU machines, transcription of a 4 h stream runs after the stream ends (≥ 4× real time ≈ 1 h).

## Validation
POC-1B runs whisper.cpp at full CPU in a worker under the Live-Green policy, during a 1080p60 dual-canvas stream. Pass: lagged frames rise by no more than 0.05 %.
