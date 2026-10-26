# ADR-0015: Observability: structured logs, trace IDs, crash reporting and opt-in telemetry

- **Status:** Accepted
- **Related:** PRD SYS-004..008, threat-model.md T-24/T-25

## Decision
- **Logging:**
  - `tracing` in every process, written as JSON lines.
  - Rotation: 10 files × 10 MB per process.
  - A `trace_id` crosses IPC with every request, so one user action can be followed through the App Host, the engine and the workers.
- **Crashes:**
  - The Crashpad handler process (`jalakam-crash.exe`) writes minidumps with stacks and the module list, without the full heap.
  - Dumps are stored locally. The user chooses whether to send them.
  - The crash backend (self-hosted or SaaS) is a release decision. The client stays backend-neutral by using the standard minidump upload format.
- **Health:** the engine stats ring feeds the health panel and the governor (ADR-0011).
- **Diagnostics bundle:**
  - Contents: redacted logs, system info, settings with secrets removed, and the latest crash dumps.
  - The file list is shown before export.
- **Telemetry:** off by default and opt-in.
  - It records counts and timings only.
  - It never records content, file names or stream URLs.

## Rejected
- **ETW-only logging:** good for deep profiling (kept for WPR sessions), but poor for bundles that users send.
- **Always-on telemetry:** conflicts with the privacy stance.
- **Full-memory dumps:** risk of exposing secrets (T-24).

## Validation
- Redaction unit tests.
- A crash test in each process must produce a dump that symbolises in CI.
