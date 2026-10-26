//! Display capture with Windows.Graphics.Capture (ADR-0007).
//!
//! The render thread owns the session and polls `TryGetNextFrame` once per
//! tick, keeping only the newest frame (latest-wins, never blocks). The frame
//! is copied into an engine-owned texture so the WGC buffer returns to the
//! pool immediately.

use anyhow::{Context, Result};
use windows::core::Interface;
use windows::Graphics::Capture::{Direct3D11CaptureFramePool, GraphicsCaptureItem, GraphicsCaptureSession};
use windows::Graphics::DirectX::Direct3D11::IDirect3DDevice;
use windows::Graphics::DirectX::DirectXPixelFormat;
use windows::Win32::Foundation::{LPARAM, RECT};
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Dxgi::IDXGIDevice;
use windows::Win32::Graphics::Gdi::{EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFOEXW};
use windows::Win32::System::WinRT::Direct3D11::{CreateDirect3D11DeviceFromDXGIDevice, IDirect3DDxgiInterfaceAccess};
use windows::Win32::System::WinRT::Graphics::Capture::IGraphicsCaptureItemInterop;
use windows_core::BOOL;

use crate::gpu;

#[derive(Debug, Clone, serde::Serialize)]
pub struct MonitorInfo {
    pub index: u32,
    pub name: String,
    pub width: i32,
    pub height: i32,
    pub primary: bool,
    #[serde(skip)]
    pub handle: isize,
}

pub fn list_monitors() -> Vec<MonitorInfo> {
    unsafe extern "system" fn cb(h: HMONITOR, _: HDC, _: *mut RECT, data: LPARAM) -> BOOL {
        let v = &mut *(data.0 as *mut Vec<MonitorInfo>);
        let mut mi = MONITORINFOEXW::default();
        mi.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
        if GetMonitorInfoW(h, &mut mi as *mut _ as *mut _).as_bool() {
            let r = mi.monitorInfo.rcMonitor;
            v.push(MonitorInfo {
                index: v.len() as u32,
                name: String::from_utf16_lossy(&mi.szDevice).trim_end_matches('\0').to_string(),
                width: r.right - r.left,
                height: r.bottom - r.top,
                primary: (mi.monitorInfo.dwFlags & 1) != 0,
                handle: h.0 as isize,
            });
        }
        true.into()
    }
    let mut v: Vec<MonitorInfo> = Vec::new();
    unsafe {
        let _ = EnumDisplayMonitors(None, None, Some(cb), LPARAM(&mut v as *mut _ as isize));
    }
    v
}

pub fn wgc_supported() -> bool {
    GraphicsCaptureSession::IsSupported().unwrap_or(false)
}

/// `IsBorderRequired` exists on Windows 11 / build 20348+.
pub fn wgc_borderless_supported() -> bool {
    windows::Foundation::Metadata::ApiInformation::IsPropertyPresent(
        &"Windows.Graphics.Capture.GraphicsCaptureSession".into(),
        &"IsBorderRequired".into(),
    )
    .unwrap_or(false)
}

pub struct DisplayCapture {
    pool: Direct3D11CaptureFramePool,
    session: GraphicsCaptureSession,
    pub texture: ID3D11Texture2D,
    pub srv: ID3D11ShaderResourceView,
    pub width: u32,
    pub height: u32,
    pub frames: u64,
    pub name: String,
}

impl DisplayCapture {
    pub fn start(dev: &ID3D11Device, monitor_index: u32) -> Result<Self> {
        anyhow::ensure!(wgc_supported(), "CAP.DISPLAY.UNSUPPORTED: Windows.Graphics.Capture not supported");
        let mons = list_monitors();
        let m = mons
            .iter()
            .find(|m| m.index == monitor_index)
            .with_context(|| format!("CAP.DISPLAY.NOT_FOUND: monitor {monitor_index} (have {})", mons.len()))?;
        unsafe {
            let interop = windows::core::factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()?;
            let item: GraphicsCaptureItem = interop.CreateForMonitor(HMONITOR(m.handle as *mut _))?;
            let size = item.Size()?;
            let dxgi: IDXGIDevice = dev.cast()?;
            let rt_dev: IDirect3DDevice = CreateDirect3D11DeviceFromDXGIDevice(&dxgi)?.cast()?;
            let pool = Direct3D11CaptureFramePool::CreateFreeThreaded(&rt_dev, DirectXPixelFormat::B8G8R8A8UIntNormalized, 2, size)?;
            let session = pool.CreateCaptureSession(&item)?;
            if wgc_borderless_supported() {
                let _ = session.SetIsBorderRequired(false);
            }
            let _ = session.SetIsCursorCaptureEnabled(true);
            session.StartCapture()?;
            let (w, h) = (size.Width as u32, size.Height as u32);
            let texture = gpu::create_texture(
                dev,
                &gpu::texture_desc(w, h, DXGI_FORMAT_B8G8R8A8_UNORM, D3D11_BIND_SHADER_RESOURCE, D3D11_RESOURCE_MISC_FLAG(0)),
            )?;
            let srv = gpu::create_srv(dev, &texture)?;
            Ok(Self { pool, session, texture, srv, width: w, height: h, frames: 0, name: m.name.clone() })
        }
    }

    /// Copy the newest captured frame, if any, into `self.texture`.
    pub fn poll(&mut self, ctx: &ID3D11DeviceContext) -> Result<bool> {
        let mut latest = None;
        while let Ok(f) = self.pool.TryGetNextFrame() {
            latest = Some(f);
        }
        let Some(frame) = latest else { return Ok(false) };
        let size = frame.ContentSize()?;
        let surface = frame.Surface()?;
        let access: IDirect3DDxgiInterfaceAccess = surface.cast()?;
        let src: ID3D11Texture2D = unsafe { access.GetInterface()? };
        let mut desc = D3D11_TEXTURE2D_DESC::default();
        unsafe { src.GetDesc(&mut desc) };
        if desc.Width == self.width && desc.Height == self.height {
            unsafe { ctx.CopyResource(&self.texture, &src) };
        }
        let _ = frame.Close();
        // Monitor resolution changed: recreate pool and our texture next time.
        if size.Width as u32 != self.width || size.Height as u32 != self.height {
            let dev = unsafe { ctx.GetDevice()? };
            let rt_dev: IDirect3DDevice = unsafe { CreateDirect3D11DeviceFromDXGIDevice(&dev.cast::<IDXGIDevice>()?)?.cast()? };
            self.pool.Recreate(&rt_dev, DirectXPixelFormat::B8G8R8A8UIntNormalized, 2, size)?;
            self.width = size.Width as u32;
            self.height = size.Height as u32;
            self.texture = gpu::create_texture(
                &dev,
                &gpu::texture_desc(self.width, self.height, DXGI_FORMAT_B8G8R8A8_UNORM, D3D11_BIND_SHADER_RESOURCE, D3D11_RESOURCE_MISC_FLAG(0)),
            )?;
            self.srv = gpu::create_srv(&dev, &self.texture)?;
        }
        self.frames += 1;
        Ok(true)
    }
}

impl Drop for DisplayCapture {
    fn drop(&mut self) {
        let _ = self.session.Close();
        let _ = self.pool.Close();
    }
}
