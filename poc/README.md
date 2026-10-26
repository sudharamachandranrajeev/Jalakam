# Phase 4 POC: media engine and streaming spine

A small, real end-to-end pipeline that tests the riskiest Phase 2 decisions (ADR-0004, 0006, 0007, 0008 and 0014) on real hardware. This is throwaway proof-of-concept code. It is not the product scaffold.

Results, limitations and architecture changes: [`docs/architecture/poc-1-results.md`](../docs/architecture/poc-1-results.md).

## What it does

```
WGC display ──┐                                            ┌─ NVENC 1280x720 ─ FLV ─ RTMP ─ MediaMTX (127.0.0.1)
MF camera ────┼─ D3D11 compositor 1920x1080@60 ─ NV12 ─────┤
D2D text ─────┘   (render thread, QPC clock)               └─ NVENC 1920x1080 ─ MKV recording
                    │
                    └─ preview ring (3 shared keyed-mutex textures) ─ jalakam-preview-poc (separate process)
WASAPI mic ─ mixer (QPC, 1024-sample frames) ─ AAC ─ fanned out to both outputs
```

| Crate | Role |
|---|---|
| `apps/jalakam-engine-poc` | Engine process: capture, compositor, encoders, outputs, stats, and a named-pipe control server |
| `apps/jalakam-preview-poc` | Preview process: Win32 window, D3D11 on the engine's adapter, letterboxed flip-model present |
| `apps/jkctl` | Command-line control client (JSON lines over `\\.\pipe\jalakam-poc-engine`) |
| `crates/jkp-proto` | Control protocol, stats types, preview-ring layout |

Design points the tests check:

- **No blocking on the UI.** The preview UI thread only pumps messages. GPU work runs on a render thread and pipe I/O on a control thread.
- **No blocking on the render thread.**
  - Sources hand over frames through latest-wins mailboxes.
  - Keyed mutexes use a 0 ms timeout.
  - Encoder input queues are bounded at 3 frames; a full queue drops the frame and counts it.
  - Output queues are bounded by seconds of media. When one overflows, that subscriber drops to the next keyframe and an IDR is forced.
- **Explicit ownership.**
  - Encoded packets are reference-counted `AVPacket`s, shared between subscribers.
  - Frames are moved, never shared.
  - Every thread has a name. On exit the engine lists any of its own threads still alive (`SHUTDOWN.THREAD_LEAK`).
- **Failures are typed.** Every component reports `state` plus `code` (`CAP.CAMERA.NOT_FOUND`, `ENC.FALLBACK`, `OUT.RTMP.WRITE_FAILED` and so on) in stats. A render-thread watchdog logs `RENDER.STALLED`.
- **Documented APIs only:** WGC, Media Foundation source reader, WASAPI, D3D11/DXGI, D2D/DirectWrite, PDH, and the FFmpeg public API.

## Prerequisites (Windows 10 22H2+ / 11, x64)

```powershell
winget install Rustlang.Rustup LLVM.LLVM BtbN.FFmpeg.LGPL.Shared.8.1 bluenviron.mediamtx
# plus Visual Studio Build Tools 2022 with the C++ workload and a Windows 11 SDK
```

FFmpeg is the BtbN **LGPL** shared build (configured with `--enable-version3`, so LGPL-3.0-or-later). The engine links its import libraries and loads its DLLs. `scripts\env.ps1` puts that `bin` first on `PATH`, so neither the engine nor the tests pick up some other ffmpeg already installed on the machine.

## Build

```powershell
cd poc
. .\scripts\env.ps1
cargo build --release
```

## Run by hand

```powershell
. .\scripts\env.ps1
& $env:JK_MEDIAMTX config\mediamtx.yml          # local RTMP server, loopback only
.\target\release\jalakam-engine-poc.exe devices  # devices, capture methods, encoder probe (JSON)
.\target\release\jalakam-engine-poc.exe run --config config\baseline.toml --stats-file out\stats.jsonl
.\target\release\jalakam-preview-poc.exe        # S stream, R record, C next camera, X stop engine, Q quit preview
.\target\release\jkctl.exe stream-start          # or: stats, record-start, camera Insta360, mic "Microphone Array", shutdown
ffplay rtmp://127.0.0.1:1935/live/test           # watch the stream (any RTMP player works)
```

`run` flags: `--stream` and `--record` start the outputs at launch, and `--duration-s N` stops cleanly after N seconds. Recordings go to `out\rec-<timestamp>.mkv`.

## Configurations

| File | Purpose |
|---|---|
| `config/baseline.toml` | Measurement baseline. Canvas 1920×1080@60. Stream: NVENC 1280×720, 4.5 Mbps CBR, 2 s GOP, to `rtmp://127.0.0.1:1935/live/test`. Record: NVENC 1920×1080, 12 Mbps, MKV. AAC 160 kbps. Display 0 plus Insta360 Link 2C camera and mic. |
| `config/invalid-devices.toml` | Camera and mic names that do not exist |
| `config/encoder-unavailable.toml` | `h264_amf` with no fallback (no AMD GPU on the test machine) |
| `config/encoder-fallback.toml` | `h264_amf` with fallback to `h264_mf` |
| `config/software-encoder.toml` | `libopenh264` for both outputs, 30 fps canvas |
| `config/mediamtx.yml` | MediaMTX: RTMP on 127.0.0.1 only, every other protocol off |

## Tests

```powershell
.\scripts\run-tests.ps1                                  # all automated tests (~50 min with the 30 min sustained run)
.\scripts\run-tests.ps1 -Tests network,leak-cycles       # a subset
.\scripts\run-tests.ps1 -Tests sustained -SustainMin 60  # longer soak
.\scripts\run-tests.ps1 -Tests device-removal            # interactive: unplug and replug the camera when asked
```

Each test writes `out\results\<run>\<test>\summary.json` (named checks and measured data), plus the engine log, the 1 Hz stats JSON lines, the MediaMTX logs and the pulled stream samples.

| Test | What it does | Pass when |
|---|---|---|
| `devices` | Enumerate adapters, monitors, cameras and formats, audio endpoints, capture methods; open each encoder for real | At least one of each; probe ≤ 5 s (ONB-002) |
| `start-stop` | 3 cycles of stream + record start/stop over the pipe; duplicate start; stop while idle; engine shutdown; process restart with a timed run | Errors `OUT.ALREADY_ACTIVE` / `OUT.NOT_ACTIVE`; every recording decodes; clean exit with no engine threads left |
| `invalid-input` | Missing camera and mic at start; missing camera selected at runtime, then recovery; malformed pipe request | `CAP.CAMERA.NOT_FOUND`, `CAP.AUDIO.NOT_FOUND`, `CTRL.BAD_REQUEST`; recording continues and decodes |
| `encoder-errors` | Unavailable encoder with no fallback; then with fallback | Start refused with `ENC.OPEN_FAILED`; fallback runs and reports `ENC.FALLBACK`; the fallback stream decodes |
| `simultaneous` | Stream and record together; pull 20 s of the stream as a player | Both decode at their own resolutions; no encoder queue drops |
| `network` | Kill MediaMTX mid-stream for about 10 s, then restart it | `OUT.RTMP.*` reported; recording unaffected; stream back to active with `reconnects ≥ 1`; stream decodes after recovery |
| `leak-cycles` | 50 output start/stop cycles, 50 camera switch cycles, 10 mic switch cycles | Robust (Theil–Sen) slope after warm-up: < 0.5 handles/cycle, < 0.2 threads/cycle, < 0.5 MB/cycle |
| `software-baseline` | 60 s on `libopenh264` | Decodes; numbers for comparison |
| `sustained` | Stream + record + preview for N minutes, with `nvidia-smi` sampling | Budgets from `performance-budgets.md` (see the results doc) |
| `device-removal` | *Interactive.* Unplug the camera during recording, then plug it back | `CAP.CAMERA.DISCONNECTED`, recording continues, camera recovers |

Not automated:
- **Permission denial.** It needs the Windows camera or microphone privacy switch turned off. Procedure: Settings → Privacy & security → Camera → "Let desktop apps access your camera" **Off**, start the engine, and expect `CAP.CAMERA.ACCESS_DENIED` in `jkctl stats`. Turn it back on afterwards.
- **Device removal.** It needs a person to unplug the camera (`-Tests device-removal`).
