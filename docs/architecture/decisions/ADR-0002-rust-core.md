# ADR-0002: Rust for the core, engine and workers

- **Status:** Accepted (Phase 2)
- **Deciders:** Opus (architecture)
- **Related:** ADR 0001 D3, technology-evaluation.md E1

## Context
The core drives Win32/COM media APIs (WGC, Media Foundation, WASAPI, D3D11, DXGI). It links FFmpeg's C API. It runs real-time threads for 4+ hours without memory growth or crashes (NFR-REL-01).

Most of the build is planned for Sonnet, with Opus on the media spine. The language must therefore surface mistakes at compile time.

## Options
1. **Rust:** the `windows` crate, `ffmpeg-sys`-style bindings, tokio for control paths.
2. **C++20:** Windows SDK headers directly, the FFmpeg C API, Qt or a custom loop.
3. **C# / .NET 8 with interop:** rejected early.
   - GC pauses on the render and audio threads need careful pinning.
   - Native interop for D3D11 shared handles and FFmpeg hwframes is a large hand-written surface.

## Decision
Rust for every process: App Host core, engine and workers.

C++ is allowed only in a thin `cc`-built shim, and only where a C++-only SDK requires it. Examples: CEF's C++ wrapper, or a vendor SDK with no C API. The shim sits behind a C ABI.

## Consequences
- **Safety:** memory and thread safety for the control plane, IPC and parsing glue. `unsafe` code stays in the FFI crates (`jk-capture`, `jk-compositor`, `jk-encode`) and gets Opus review.
- **COM cost:** COM through the `windows` crate is verbose but complete. Expect a learning cost on WGC and MF async patterns.
- **One toolchain** for Tauri, engine and workers. Shared `jk-protocol` types remove schema drift.
- **Hiring:** the pool of media specialists is smaller than for C++. Keeping the FFI layer small limits the impact.

## Rejected
- **C++20:** it has the strongest media ecosystem and the most examples. It scores lower on:
  - memory safety;
  - build reproducibility (vcpkg/CMake);
  - how reliably an LLM-assisted team avoids lifetime bugs in multithreaded media code.

  E1 score: 44 vs 49. C++ stays as the shim language.
- **C#:** GC behaviour and interop surface, as above.

## Validation
POC-1A builds the full spine in Rust: capture → D3D11 → NVENC → RTMP.

If the FFI cost for FFmpeg hwframes or WGC exceeds 2 weeks of POC time, reopen this ADR. The alternative is a C++ engine with a Rust host.
