//! Jalakam media engine proof of concept (Phase 4).
//!
//! `jalakam-engine-poc devices`              enumerate devices, capture methods, encoders
//! `jalakam-engine-poc run --config FILE`    run the engine; control over the named pipe

mod audio;
mod bus;
mod camera;
mod clock;
mod config;
mod control;
mod devices;
mod display;
mod encoder;
mod engine;
mod ff;
mod gpu;
mod mixer;
mod output;
mod overlay;
mod preview;
mod procstats;
mod quad;
mod render;
mod status;

use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use clap::{Parser, Subcommand};
use windows::Win32::Media::MediaFoundation::{MFShutdown, MFStartup, MFSTARTUP_FULL, MF_VERSION};
use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};
use windows::Win32::System::Console::SetConsoleCtrlHandler;

#[derive(Parser)]
#[command(version, about = "Jalakam media engine POC")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Print devices, capture methods and encoder availability as JSON.
    Devices {
        #[arg(long, default_value = "nvidia")]
        adapter: String,
    },
    /// Run the engine.
    Run {
        #[arg(long)]
        config: PathBuf,
        #[arg(long)]
        stream: bool,
        #[arg(long)]
        record: bool,
        /// Exit cleanly after N seconds (scripted tests).
        #[arg(long)]
        duration_s: Option<u64>,
        /// Stats JSON lines file (one snapshot per second).
        #[arg(long)]
        stats_file: Option<PathBuf>,
    },
}

static CTRL_C: AtomicBool = AtomicBool::new(false);

unsafe extern "system" fn on_ctrl(_: u32) -> windows_core::BOOL {
    CTRL_C.store(true, Ordering::SeqCst);
    true.into()
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        MFStartup(MF_VERSION, MFSTARTUP_FULL)?;
        ff::sys::avformat_network_init();
    }
    let r = match cli.cmd {
        Cmd::Devices { adapter } => {
            println!("{}", serde_json::to_string_pretty(&devices::report(&adapter))?);
            Ok(())
        }
        Cmd::Run { config, stream, record, duration_s, stats_file } => run(config, stream, record, duration_s, stats_file),
    };
    unsafe {
        let _ = MFShutdown();
    }
    r
}

fn run(config: PathBuf, stream: bool, record: bool, duration_s: Option<u64>, stats_file: Option<PathBuf>) -> Result<()> {
    let cfg = config::Config::load(&config)?;
    let base_threads = procstats::thread_count();
    let base_handles = procstats::handle_count();
    log!("ffmpeg: {} [{}]", ff::version_string(), ff::license_string());
    unsafe {
        let _ = SetConsoleCtrlHandler(Some(on_ctrl), true);
    }
    let eng = engine::Engine::start(cfg)?;
    control::serve(eng.clone())?;
    log!("control pipe: {}", jkp_proto::PIPE_NAME);
    if stream {
        if let Err(e) = eng.start_output(engine::Kind::Stream) {
            log!("stream start failed: {e:#}");
        }
    }
    if record {
        if let Err(e) = eng.start_output(engine::Kind::Record) {
            log!("record start failed: {e:#}");
        }
    }
    let mut stats_out = match &stats_file {
        Some(p) => Some(std::io::BufWriter::new(std::fs::File::create(p)?)),
        None => None,
    };
    // Render-stall watchdog on its own thread: it reads only an atomic, so it
    // still reports when the render thread or a stats lock is stuck.
    let watchdog_stop = Arc::new(AtomicBool::new(false));
    let watchdog = {
        let rs = eng.render_shared();
        let stop = watchdog_stop.clone();
        std::thread::Builder::new().name("watchdog".into()).spawn(move || {
            let (mut last, mut since, mut reported) = (rs.ticks.load(Ordering::Relaxed), Instant::now(), false);
            while !stop.load(Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(250));
                let t = rs.ticks.load(Ordering::Relaxed);
                if t != last {
                    if reported {
                        log!("RENDER.RECOVERED after {} ms", since.elapsed().as_millis());
                    }
                    (last, since, reported) = (t, Instant::now(), false);
                } else if !reported && since.elapsed() > Duration::from_secs(2) {
                    log!("RENDER.STALLED: no render tick for {} ms (tick {t})", since.elapsed().as_millis());
                    reported = true;
                }
            }
        })?
    };
    let start = Instant::now();
    let mut next_stats = Instant::now() + Duration::from_secs(1);
    while !eng.shutdown.load(Ordering::SeqCst) && !CTRL_C.load(Ordering::SeqCst) {
        if let Some(d) = duration_s {
            if start.elapsed() >= Duration::from_secs(d) {
                log!("duration reached");
                break;
            }
        }
        std::thread::sleep(Duration::from_millis(100));
        if Instant::now() >= next_stats {
            next_stats += Duration::from_secs(1);
            let snap = eng.snapshot();
            if let Some(w) = stats_out.as_mut() {
                serde_json::to_writer(&mut *w, &snap)?;
                w.write_all(b"\n")?;
                w.flush()?;
            }
            if snap.uptime_s as u64 % 10 == 0 {
                let enc: Vec<String> = snap
                    .encoders
                    .iter()
                    .map(|e| format!("{}={}({:?},in {},drop {})", e.name, e.codec, e.status.state, e.frames_in, e.dropped_queue_full))
                    .collect();
                let out: Vec<String> = snap
                    .outputs
                    .iter()
                    .map(|o| format!("{}={:?} {:.0}kbps drop {} rc {}", o.name, o.status.state, o.kbps, o.dropped_packets, o.reconnects))
                    .collect();
                log!(
                    "t={:.0}s ticks {} lagged {} gpu {:.2}ms cpu {:.1}% ws {:.0}MB enc3d {:.1}% vidEnc {:.1}% | {} | {}",
                    snap.uptime_s,
                    snap.render.ticks,
                    snap.render.lagged,
                    snap.render.gpu_ms_avg,
                    snap.process.cpu_pct_of_machine,
                    snap.process.working_set_mb,
                    snap.process.gpu_3d_pct,
                    snap.process.gpu_video_encode_pct,
                    enc.join(" "),
                    out.join(" ")
                );
            }
        }
    }
    watchdog_stop.store(true, Ordering::SeqCst);
    let _ = watchdog.join();
    log!("shutting down; live named threads: {:?}", procstats::named_threads().0);
    let final_snap = eng.snapshot();
    let took = eng.shutdown_all();
    if let Some(w) = stats_out.as_mut() {
        serde_json::to_writer(&mut *w, &final_snap)?;
        w.write_all(b"\n")?;
        w.flush()?;
    }
    let threads = procstats::thread_count();
    let handles = procstats::handle_count();
    let (named, unnamed) = procstats::named_threads();
    // Only names this engine gives its own threads count; other named
    // threads are injected by third-party tools on the machine. The control
    // pipe's accept thread may legitimately outlive shutdown.
    const OURS: &[&str] = &["cam", "mic", "mixer", "render", "ctrl-client", "watchdog"];
    let leaked: Vec<_> = named.iter().filter(|n| OURS.contains(&n.as_str()) || n.starts_with("enc-") || n.starts_with("out-")).collect();
    log!(
        "shutdown in {} ms: threads {} (baseline {}; engine threads still alive: {:?}; other threads: {} unnamed (OS/MF/driver) + {} foreign), handles {} (baseline {})",
        took.as_millis(),
        threads,
        base_threads,
        leaked,
        unnamed,
        named.len() - leaked.len(),
        handles,
        base_handles
    );
    if !leaked.is_empty() {
        log!("SHUTDOWN.THREAD_LEAK: {leaked:?}");
    }
    if let Some(p) = eng.last_record_path.lock().as_ref() {
        log!("last recording: {p}");
    }
    Ok(())
}
