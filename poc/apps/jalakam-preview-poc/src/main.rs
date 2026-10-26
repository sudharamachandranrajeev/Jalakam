//! Jalakam POC preview: a separate process that shows the engine's canvas
//! (ADR-0008 preview ring) with correct aspect ratio.
//!
//! Threads, so the window never waits on the GPU or the engine:
//! - UI thread: Win32 message loop and title updates only.
//! - render thread: D3D11 device on the engine's adapter (LUID from the
//!   shared meta), keyed-mutex copy of the latest slot with a 0 ms timeout,
//!   letterboxed draw, flip-model present.
//! - control thread: named-pipe client; polls stats once a second and sends
//!   key commands (S stream, R record, C next camera, X engine shutdown).
//!   Q or closing the window exits the preview only.

#[path = "../../jalakam-engine-poc/src/quad.rs"]
mod quad;

use std::io::{BufRead, BufReader, Write};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use jkp_proto::{preview_texture_name, Envelope, PreviewMeta, Request, Response, PIPE_NAME, PREVIEW_META_NAME, PREVIEW_SLOTS, PROTOCOL_VERSION};
use quad::{QuadPipeline, Rect};
use windows::core::{w, Interface, HSTRING};
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Direct3D::*;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::Graphics::Dxgi::*;
use windows::Win32::Graphics::Gdi::{GetStockObject, BLACK_BRUSH, HBRUSH};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Memory::*;
use windows::Win32::UI::HiDpi::{SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2};
use windows::Win32::UI::WindowsAndMessaging::*;

const TIMER_TITLE: usize = 1;

/// Shared between the three threads.
struct App {
    quit: AtomicBool,
    /// Client size packed as (w << 32) | h; written by the UI thread.
    size: AtomicU64,
    /// Title text produced by the render and control threads.
    render_status: Mutex<String>,
    engine_status: Mutex<String>,
    cmd_tx: Mutex<Option<mpsc::Sender<Cmd>>>,
}

enum Cmd {
    ToggleStream,
    ToggleRecord,
    NextCamera,
    Shutdown,
}

static APP: OnceLock<Arc<App>> = OnceLock::new();

fn log(msg: &str) {
    eprintln!("[preview] {msg}");
}

fn main() -> Result<()> {
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
    let (tx, rx) = mpsc::channel();
    let app = Arc::new(App {
        quit: AtomicBool::new(false),
        size: AtomicU64::new((960u64 << 32) | 540),
        render_status: Mutex::new("waiting for engine".into()),
        engine_status: Mutex::new(String::new()),
        cmd_tx: Mutex::new(Some(tx)),
    });
    let _ = APP.set(app.clone());

    let hwnd = create_window()?;
    let render = {
        let app = app.clone();
        let h = hwnd.0 as usize;
        std::thread::Builder::new().name("preview-render".into()).spawn(move || {
            let hwnd = HWND(h as *mut _);
            while !app.quit.load(Ordering::SeqCst) {
                if let Err(e) = render_loop(&app, hwnd) {
                    *app.render_status.lock().unwrap() = format!("render: {e:#}");
                    log(&format!("render loop restart: {e:#}"));
                    sleep_unless_quit(&app, Duration::from_secs(1));
                }
            }
        })?
    };
    let control = {
        let app = app.clone();
        std::thread::Builder::new().name("preview-ctrl".into()).spawn(move || control_loop(&app, rx))?
    };

    unsafe {
        let _ = ShowWindow(hwnd, SW_SHOW);
        SetTimer(Some(hwnd), TIMER_TITLE, 250, None);
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    app.quit.store(true, Ordering::SeqCst);
    app.cmd_tx.lock().unwrap().take(); // closes the control channel
    let _ = render.join();
    let _ = control.join();
    log("exited cleanly");
    Ok(())
}

fn sleep_unless_quit(app: &App, d: Duration) {
    let end = Instant::now() + d;
    while Instant::now() < end && !app.quit.load(Ordering::SeqCst) {
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn create_window() -> Result<HWND> {
    unsafe {
        let inst: HINSTANCE = GetModuleHandleW(None)?.into();
        let class = w!("JalakamPreviewPoc");
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(wndproc),
            hInstance: inst,
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            hbrBackground: HBRUSH(GetStockObject(BLACK_BRUSH).0),
            lpszClassName: class,
            ..Default::default()
        };
        anyhow::ensure!(RegisterClassExW(&wc) != 0, "RegisterClassExW");
        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            class,
            w!("Jalakam preview"),
            WS_OVERLAPPEDWINDOW,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            1000,
            600,
            None,
            None,
            Some(inst),
            None,
        )?;
        Ok(hwnd)
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    let Some(app) = APP.get() else { return DefWindowProcW(hwnd, msg, wp, lp) };
    match msg {
        WM_SIZE => {
            let w = (lp.0 as u32 & 0xFFFF).max(1) as u64;
            let h = ((lp.0 as u32 >> 16) & 0xFFFF).max(1) as u64;
            app.size.store((w << 32) | h, Ordering::Release);
            LRESULT(0)
        }
        WM_KEYDOWN => {
            let cmd = match wp.0 as u8 {
                b'S' => Some(Cmd::ToggleStream),
                b'R' => Some(Cmd::ToggleRecord),
                b'C' => Some(Cmd::NextCamera),
                b'X' => Some(Cmd::Shutdown),
                b'Q' => {
                    let _ = DestroyWindow(hwnd);
                    None
                }
                _ => None,
            };
            if let (Some(c), Some(tx)) = (cmd, app.cmd_tx.lock().unwrap().as_ref()) {
                let _ = tx.send(c);
            }
            LRESULT(0)
        }
        WM_TIMER => {
            let title = format!(
                "Jalakam preview | {} | {} | S stream  R record  C camera  X stop engine  Q quit",
                app.render_status.lock().unwrap(),
                app.engine_status.lock().unwrap()
            );
            let _ = SetWindowTextW(hwnd, &HSTRING::from(title));
            LRESULT(0)
        }
        WM_ERASEBKGND => LRESULT(1),
        WM_DESTROY => {
            app.quit.store(true, Ordering::SeqCst);
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

// ---------------------------------------------------------------- render

struct Meta {
    map: HANDLE,
    view: MEMORY_MAPPED_VIEW_ADDRESS,
}

impl Meta {
    fn open() -> Result<Self> {
        unsafe {
            let map = OpenFileMappingW(FILE_MAP_READ.0, false, &HSTRING::from(PREVIEW_META_NAME)).context("engine preview not available")?;
            let view = MapViewOfFile(map, FILE_MAP_READ, 0, 0, std::mem::size_of::<PreviewMeta>());
            if view.Value.is_null() {
                let _ = CloseHandle(map);
                anyhow::bail!("MapViewOfFile (preview meta)");
            }
            Ok(Self { map, view })
        }
    }
    fn get(&self) -> &PreviewMeta {
        unsafe { &*(self.view.Value as *const PreviewMeta) }
    }
    /// Consistent (slot, frame) pair under the engine's sequence lock.
    fn latest(&self) -> Option<(usize, u64)> {
        let m = self.get();
        for _ in 0..4 {
            let s1 = m.seq.load(Ordering::Acquire);
            if s1 & 1 == 1 {
                continue;
            }
            let slot = m.latest_slot.load(Ordering::Relaxed) as usize;
            let frame = m.frame_number.load(Ordering::Relaxed);
            if m.seq.load(Ordering::Acquire) == s1 {
                return Some((slot, frame));
            }
        }
        None
    }
}

impl Drop for Meta {
    fn drop(&mut self) {
        unsafe {
            let _ = UnmapViewOfFile(self.view);
            let _ = CloseHandle(self.map);
        }
    }
}

struct Slots {
    generation: u32,
    width: u32,
    height: u32,
    shared: Vec<(ID3D11Texture2D, IDXGIKeyedMutex)>,
    local: ID3D11Texture2D,
    local_srv: ID3D11ShaderResourceView,
}

/// See the engine's `preview::try_acquire`: WAIT_TIMEOUT is a success HRESULT.
fn try_acquire(m: &IDXGIKeyedMutex, key: u64) -> bool {
    unsafe { (Interface::vtable(m).AcquireSync)(Interface::as_raw(m), key, 0).0 == 0 }
}

fn open_slots(dev: &ID3D11Device, meta: &PreviewMeta) -> Result<Slots> {
    let dev1: ID3D11Device1 = dev.cast()?;
    let generation = meta.generation.load(Ordering::Acquire);
    let (width, height) = (meta.width.load(Ordering::Relaxed), meta.height.load(Ordering::Relaxed));
    let mut shared = Vec::new();
    for i in 0..PREVIEW_SLOTS {
        let name = HSTRING::from(preview_texture_name(i));
        let tex: ID3D11Texture2D = unsafe {
            dev1.OpenSharedResourceByName(&name, DXGI_SHARED_RESOURCE_READ.0 | DXGI_SHARED_RESOURCE_WRITE.0)
                .with_context(|| format!("OpenSharedResourceByName slot {i}"))?
        };
        let km: IDXGIKeyedMutex = tex.cast()?;
        shared.push((tex, km));
    }
    let desc = D3D11_TEXTURE2D_DESC {
        Width: width,
        Height: height,
        MipLevels: 1,
        ArraySize: 1,
        Format: DXGI_FORMAT_B8G8R8A8_UNORM,
        SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
        Usage: D3D11_USAGE_DEFAULT,
        BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as u32,
        CPUAccessFlags: 0,
        MiscFlags: 0,
    };
    let mut local = None;
    let mut srv = None;
    unsafe {
        dev.CreateTexture2D(&desc, None, Some(&mut local))?;
        dev.CreateShaderResourceView(local.as_ref().unwrap(), None, Some(&mut srv))?;
    }
    Ok(Slots { generation, width, height, shared, local: local.unwrap(), local_srv: srv.unwrap() })
}

fn create_device(luid: LUID) -> Result<(ID3D11Device, ID3D11DeviceContext, String)> {
    unsafe {
        let factory: IDXGIFactory4 = CreateDXGIFactory2(DXGI_CREATE_FACTORY_FLAGS(0))?;
        let adapter: IDXGIAdapter1 = factory.EnumAdapterByLuid(luid).context("engine adapter not found by LUID")?;
        let name = String::from_utf16_lossy(&adapter.GetDesc1()?.Description).trim_end_matches('\0').to_string();
        let mut dev = None;
        let mut ctx = None;
        D3D11CreateDevice(
            &adapter,
            D3D_DRIVER_TYPE_UNKNOWN,
            HMODULE::default(),
            D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            Some(&[D3D_FEATURE_LEVEL_11_1, D3D_FEATURE_LEVEL_11_0]),
            D3D11_SDK_VERSION,
            Some(&mut dev),
            None,
            Some(&mut ctx),
        )?;
        Ok((dev.unwrap(), ctx.unwrap(), name))
    }
}

fn render_loop(app: &App, hwnd: HWND) -> Result<()> {
    // Wait for the engine's meta section.
    let meta = loop {
        if app.quit.load(Ordering::SeqCst) {
            return Ok(());
        }
        match Meta::open() {
            Ok(m) if m.get().generation.load(Ordering::Acquire) > 0 => break m,
            _ => {
                *app.render_status.lock().unwrap() = "waiting for engine preview".into();
                sleep_unless_quit(app, Duration::from_millis(500));
            }
        }
    };
    let luid = LUID { LowPart: meta.get().adapter_luid_low.load(Ordering::Relaxed), HighPart: meta.get().adapter_luid_high.load(Ordering::Relaxed) };
    let (dev, ctx, adapter) = create_device(luid)?;
    log(&format!("device on {adapter} (same adapter as engine, by LUID)"));
    let quad = QuadPipeline::new(&dev)?;

    let factory: IDXGIFactory2 = unsafe { CreateDXGIFactory2(DXGI_CREATE_FACTORY_FLAGS(0))? };
    let packed = app.size.load(Ordering::Acquire);
    let (mut sw, mut sh) = ((packed >> 32) as u32, packed as u32);
    let scd = DXGI_SWAP_CHAIN_DESC1 {
        Width: sw,
        Height: sh,
        Format: DXGI_FORMAT_B8G8R8A8_UNORM,
        SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
        BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
        BufferCount: 2,
        Scaling: DXGI_SCALING_NONE,
        SwapEffect: DXGI_SWAP_EFFECT_FLIP_DISCARD,
        AlphaMode: DXGI_ALPHA_MODE_IGNORE,
        Flags: 0,
        ..Default::default()
    };
    let swap = unsafe { factory.CreateSwapChainForHwnd(&dev, hwnd, &scd, None, None)? };
    unsafe {
        let _ = factory.MakeWindowAssociation(hwnd, DXGI_MWA_NO_ALT_ENTER);
    }
    let make_rtv = |swap: &IDXGISwapChain1| -> Result<ID3D11RenderTargetView> {
        let bb: ID3D11Texture2D = unsafe { swap.GetBuffer(0)? };
        let mut rtv = None;
        unsafe { dev.CreateRenderTargetView(&bb, None, Some(&mut rtv))? };
        Ok(rtv.unwrap())
    };
    let mut rtv = Some(make_rtv(&swap)?);

    let mut slots: Option<Slots> = None;
    let mut have_frame = false;
    let mut last_frame = 0u64;
    let mut last_new_frame_at = Instant::now();
    // Per-second counters for the title and log.
    let (mut copies, mut busy, mut presents) = (0u32, 0u32, 0u32);
    let mut worst_present_gap = Duration::ZERO;
    let mut last_present = Instant::now();
    let mut window_start = Instant::now();

    while !app.quit.load(Ordering::SeqCst) {
        let packed = app.size.load(Ordering::Acquire);
        let (w, h) = ((packed >> 32) as u32, packed as u32);
        if (w, h) != (sw, sh) && w > 0 && h > 0 {
            rtv = None;
            unsafe {
                ctx.OMSetRenderTargets(None, None);
                ctx.Flush();
                swap.ResizeBuffers(0, w, h, DXGI_FORMAT_UNKNOWN, DXGI_SWAP_CHAIN_FLAG(0))?;
            }
            rtv = Some(make_rtv(&swap)?);
            (sw, sh) = (w, h);
        }

        let m = meta.get();
        if slots.as_ref().map(|s| s.generation) != Some(m.generation.load(Ordering::Acquire)) {
            slots = None;
            have_frame = false;
            match open_slots(&dev, m) {
                Ok(s) => {
                    log(&format!("opened preview ring generation {} ({}x{})", s.generation, s.width, s.height));
                    slots = Some(s);
                }
                Err(e) => *app.render_status.lock().unwrap() = format!("preview ring: {e:#}"),
            }
        }

        if let (Some(s), Some((slot, frame))) = (slots.as_ref(), meta.latest()) {
            if frame != last_frame && slot < s.shared.len() {
                let (tex, km) = &s.shared[slot];
                if try_acquire(km, 1) {
                    unsafe {
                        ctx.CopyResource(&s.local, tex);
                        let _ = km.ReleaseSync(0);
                    }
                    last_frame = frame;
                    have_frame = true;
                    copies += 1;
                    last_new_frame_at = Instant::now();
                } else {
                    busy += 1; // engine is writing that slot; take the next one
                }
            }
        }

        let rt = rtv.as_ref().unwrap();
        quad.begin(&ctx, rt, sw, sh, [0.08, 0.08, 0.09, 1.0]);
        if let (true, Some(s)) = (have_frame, slots.as_ref()) {
            let dst = Rect::fit(s.width as f32, s.height as f32, Rect { x: 0.0, y: 0.0, w: sw as f32, h: sh as f32 });
            quad.draw(&ctx, &s.local_srv, dst, sw, sh, 1.0, true)?;
        }
        unsafe {
            ctx.OMSetRenderTargets(None, None);
            let hr = swap.Present(1, DXGI_PRESENT(0));
            if hr.is_err() {
                anyhow::bail!("Present: {hr:?}");
            }
        }
        presents += 1;
        let now = Instant::now();
        worst_present_gap = worst_present_gap.max(now - last_present);
        last_present = now;

        if now - window_start >= Duration::from_secs(1) {
            let secs = (now - window_start).as_secs_f64();
            let stale = now - last_new_frame_at > Duration::from_secs(2);
            let status = format!(
                "{}{:.0} fps copied, {:.0} fps presented, {} busy, worst gap {:.1} ms",
                if stale { "STALE (no new engine frames) | " } else { "" },
                copies as f64 / secs,
                presents as f64 / secs,
                busy,
                worst_present_gap.as_secs_f64() * 1000.0
            );
            log(&status);
            *app.render_status.lock().unwrap() = status;
            (copies, busy, presents) = (0, 0, 0);
            worst_present_gap = Duration::ZERO;
            window_start = now;
        }
    }
    Ok(())
}

// --------------------------------------------------------------- control

struct Pipe {
    reader: BufReader<std::fs::File>,
    writer: std::fs::File,
    next_id: u64,
}

impl Pipe {
    fn connect() -> Result<Self> {
        let f = std::fs::OpenOptions::new().read(true).write(true).open(PIPE_NAME)?;
        let writer = f.try_clone()?;
        let mut p = Self { reader: BufReader::new(f), writer, next_id: 1 };
        p.call(Request::Hello { version: PROTOCOL_VERSION })?;
        Ok(p)
    }
    fn call(&mut self, req: Request) -> Result<serde_json::Value> {
        let id = self.next_id;
        self.next_id += 1;
        let mut line = serde_json::to_vec(&Envelope { id, body: req })?;
        line.push(b'\n');
        self.writer.write_all(&line)?;
        let mut resp = String::new();
        anyhow::ensure!(self.reader.read_line(&mut resp)? > 0, "engine closed the pipe");
        match serde_json::from_str::<Envelope<Response>>(&resp)?.body {
            Response::Ok { result } => Ok(result),
            Response::Error { code, message } => anyhow::bail!("{code}: {message}"),
        }
    }
}

fn state_of(stats: &serde_json::Value, name: &str) -> String {
    stats["outputs"]
        .as_array()
        .and_then(|a| a.iter().find(|o| o["name"] == name))
        .and_then(|o| o["status"]["state"].as_str())
        .unwrap_or("idle")
        .to_string()
}

fn control_loop(app: &App, rx: mpsc::Receiver<Cmd>) {
    let mut pipe: Option<Pipe> = None;
    let mut stats = serde_json::Value::Null;
    let mut camera_cycle = 0usize;
    let mut next_poll = Instant::now();
    loop {
        let cmd = match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(c) => Some(c),
            Err(mpsc::RecvTimeoutError::Timeout) => None,
            Err(mpsc::RecvTimeoutError::Disconnected) => return,
        };
        if pipe.is_none() {
            match Pipe::connect() {
                Ok(p) => pipe = Some(p),
                Err(_) => {
                    *app.engine_status.lock().unwrap() = "engine: not connected".into();
                    continue;
                }
            }
        }
        let p = pipe.as_mut().unwrap();
        let result = (|| -> Result<Option<String>> {
            if let Some(cmd) = cmd {
                let req = match cmd {
                    Cmd::ToggleStream => {
                        if state_of(&stats, "stream") == "idle" { Request::StreamStart } else { Request::StreamStop }
                    }
                    Cmd::ToggleRecord => {
                        if state_of(&stats, "record") == "idle" { Request::RecordStart } else { Request::RecordStop }
                    }
                    Cmd::NextCamera => {
                        camera_cycle = (camera_cycle + 1) % 4;
                        Request::CameraSelect { device: if camera_cycle == 3 { None } else { Some(camera_cycle.to_string()) } }
                    }
                    Cmd::Shutdown => Request::Shutdown,
                };
                let label = format!("{req:?}");
                let r = p.call(req);
                log(&format!("{label} -> {}", match &r { Ok(v) => v.to_string(), Err(e) => format!("{e:#}") }));
                if let Err(e) = r {
                    return Ok(Some(format!("last command failed: {e:#}")));
                }
                next_poll = Instant::now();
            }
            if Instant::now() >= next_poll {
                next_poll = Instant::now() + Duration::from_secs(1);
                stats = p.call(Request::Stats)?;
            }
            Ok(None)
        })();
        match result {
            Ok(note) => {
                let cam = &stats["camera"];
                let mut s = format!(
                    "stream {} | record {} | camera {} ({})",
                    state_of(&stats, "stream"),
                    state_of(&stats, "record"),
                    cam["name"].as_str().unwrap_or("-"),
                    cam["status"]["state"].as_str().unwrap_or("none"),
                );
                if let Some(n) = note {
                    s.push_str(" | ");
                    s.push_str(&n);
                }
                *app.engine_status.lock().unwrap() = s;
            }
            Err(e) => {
                *app.engine_status.lock().unwrap() = format!("engine: {e:#}");
                pipe = None;
            }
        }
    }
}

