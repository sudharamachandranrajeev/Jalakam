//! D3D11 device creation and adapter selection (ADR-0006).

use anyhow::{bail, Context, Result};
use windows::core::Interface;
use windows::Win32::Foundation::{HMODULE, LUID};
use windows::Win32::Graphics::Direct3D::*;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::*;
use windows::Win32::Graphics::Dxgi::Common::*;

pub struct Gpu {
    pub device: ID3D11Device,
    pub context: ID3D11DeviceContext,
    pub adapter_name: String,
    pub adapter_luid: LUID,
    pub vendor_id: u32,
}

// The device is created with multithread protection on, so the COM pointers
// may be used from more than one thread (MF, FFmpeg's NVENC wrapper).
unsafe impl Send for Gpu {}
unsafe impl Sync for Gpu {}

pub struct AdapterInfo {
    pub index: u32,
    pub name: String,
    pub vendor_id: u32,
    pub dedicated_mb: u64,
    pub luid: LUID,
    pub software: bool,
}

pub fn list_adapters() -> Result<Vec<AdapterInfo>> {
    let mut out = Vec::new();
    unsafe {
        let factory: IDXGIFactory1 = CreateDXGIFactory1()?;
        let mut i = 0;
        while let Ok(a) = factory.EnumAdapters1(i) {
            let d = a.GetDesc1()?;
            let name = String::from_utf16_lossy(&d.Description).trim_end_matches('\0').to_string();
            out.push(AdapterInfo {
                index: i,
                name,
                vendor_id: d.VendorId,
                dedicated_mb: d.DedicatedVideoMemory as u64 / (1024 * 1024),
                luid: d.AdapterLuid,
                software: (d.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32) != 0,
            });
            i += 1;
        }
    }
    Ok(out)
}

pub fn vendor_name(id: u32) -> &'static str {
    match id {
        0x10DE => "nvidia",
        0x1002 => "amd",
        0x8086 => "intel",
        _ => "other",
    }
}

impl Gpu {
    pub fn create(preferred_vendor: &str) -> Result<Self> {
        let adapters = list_adapters()?;
        let pick = adapters
            .iter()
            .find(|a| !a.software && (preferred_vendor == "any" || vendor_name(a.vendor_id) == preferred_vendor))
            .or_else(|| adapters.iter().find(|a| !a.software))
            .context("no hardware DXGI adapter found")?;
        unsafe {
            let factory: IDXGIFactory1 = CreateDXGIFactory1()?;
            let adapter: IDXGIAdapter1 = factory.EnumAdapters1(pick.index)?;
            let mut device = None;
            let mut context = None;
            let flags = D3D11_CREATE_DEVICE_BGRA_SUPPORT | D3D11_CREATE_DEVICE_VIDEO_SUPPORT;
            let levels = [D3D_FEATURE_LEVEL_11_1, D3D_FEATURE_LEVEL_11_0];
            D3D11CreateDevice(
                &adapter,
                D3D_DRIVER_TYPE_UNKNOWN,
                HMODULE::default(),
                flags,
                Some(&levels),
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                Some(&mut context),
            )
            .context("D3D11CreateDevice")?;
            let device: ID3D11Device = device.unwrap();
            let context: ID3D11DeviceContext = context.unwrap();
            let mt: ID3D11Multithread = context.cast()?;
            let _ = mt.SetMultithreadProtected(true);
            // Real-time engine: highest GPU scheduling priority (ADR-0011).
            if let Ok(dxgi_dev) = device.cast::<IDXGIDevice>() {
                let _ = dxgi_dev.SetGPUThreadPriority(7);
            }
            if device.GetFeatureLevel().0 < D3D_FEATURE_LEVEL_11_0.0 {
                bail!("feature level 11_0 required");
            }
            Ok(Self {
                device,
                context,
                adapter_name: pick.name.clone(),
                adapter_luid: pick.luid,
                vendor_id: pick.vendor_id,
            })
        }
    }
}

pub fn texture_desc(w: u32, h: u32, format: DXGI_FORMAT, bind: D3D11_BIND_FLAG, misc: D3D11_RESOURCE_MISC_FLAG) -> D3D11_TEXTURE2D_DESC {
    D3D11_TEXTURE2D_DESC {
        Width: w,
        Height: h,
        MipLevels: 1,
        ArraySize: 1,
        Format: format,
        SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
        Usage: D3D11_USAGE_DEFAULT,
        BindFlags: bind.0 as u32,
        CPUAccessFlags: 0,
        MiscFlags: misc.0 as u32,
    }
}

pub fn create_texture(dev: &ID3D11Device, desc: &D3D11_TEXTURE2D_DESC) -> Result<ID3D11Texture2D> {
    let mut t = None;
    unsafe { dev.CreateTexture2D(desc, None, Some(&mut t))? };
    Ok(t.unwrap())
}

pub fn create_srv(dev: &ID3D11Device, tex: &ID3D11Texture2D) -> Result<ID3D11ShaderResourceView> {
    let mut v = None;
    unsafe { dev.CreateShaderResourceView(tex, None, Some(&mut v))? };
    Ok(v.unwrap())
}

pub fn create_rtv(dev: &ID3D11Device, tex: &ID3D11Texture2D) -> Result<ID3D11RenderTargetView> {
    let mut v = None;
    unsafe { dev.CreateRenderTargetView(tex, None, Some(&mut v))? };
    Ok(v.unwrap())
}
