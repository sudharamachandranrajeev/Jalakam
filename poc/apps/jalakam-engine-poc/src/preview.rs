//! Preview producer: a ring of shared, keyed-mutex textures (ADR-0008).
//!
//! Key protocol per slot: the engine writes after acquiring key 0 (slot free
//! or consumed) or key 1 (slot written but not yet consumed), then releases
//! key 1. The preview process acquires key 1, copies the slot into its own
//! texture and releases key 0. All acquires use a 0 ms timeout, so neither
//! side ever waits for the other.

use std::sync::atomic::Ordering;

use anyhow::{Context, Result};
use jkp_proto::{preview_texture_name, PreviewMeta, PREVIEW_META_NAME, PREVIEW_SLOTS};
use windows::core::{Interface, HSTRING};
use windows::Win32::Foundation::{CloseHandle, GENERIC_ALL, HANDLE, INVALID_HANDLE_VALUE, LUID};
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Dxgi::{IDXGIKeyedMutex, IDXGIResource1, DXGI_SHARED_RESOURCE_READ, DXGI_SHARED_RESOURCE_WRITE};
use windows::Win32::System::Memory::*;

use crate::gpu;
use crate::quad::{QuadPipeline, Rect};

struct Slot {
    rtv: ID3D11RenderTargetView,
    mutex: IDXGIKeyedMutex,
    handle: HANDLE,
    _tex: ID3D11Texture2D,
}

pub struct PreviewRing {
    slots: Vec<Slot>,
    map: HANDLE,
    view: MEMORY_MAPPED_VIEW_ADDRESS,
    next: usize,
    pub width: u32,
    pub height: u32,
    pub skipped: u64,
    frame: u64,
}

/// `IDXGIKeyedMutex::AcquireSync` returns WAIT_TIMEOUT as a *success*
/// HRESULT, which the generated wrapper folds into Ok. Call the vtable to see it.
pub fn try_acquire(m: &IDXGIKeyedMutex, key: u64) -> bool {
    unsafe {
        let hr = (Interface::vtable(m).AcquireSync)(Interface::as_raw(m), key, 0);
        hr.0 == 0
    }
}

impl PreviewRing {
    pub fn new(dev: &ID3D11Device, width: u32, height: u32, luid: LUID) -> Result<Self> {
        let mut slots = Vec::new();
        for i in 0..PREVIEW_SLOTS {
            let desc = gpu::texture_desc(
                width,
                height,
                DXGI_FORMAT_B8G8R8A8_UNORM,
                D3D11_BIND_FLAG(D3D11_BIND_RENDER_TARGET.0 | D3D11_BIND_SHADER_RESOURCE.0),
                D3D11_RESOURCE_MISC_FLAG(D3D11_RESOURCE_MISC_SHARED_NTHANDLE.0 | D3D11_RESOURCE_MISC_SHARED_KEYEDMUTEX.0),
            );
            let tex = gpu::create_texture(dev, &desc)?;
            let rtv = gpu::create_rtv(dev, &tex)?;
            let res: IDXGIResource1 = tex.cast()?;
            let name = HSTRING::from(preview_texture_name(i));
            let handle = unsafe {
                res.CreateSharedHandle(None, DXGI_SHARED_RESOURCE_READ.0 | DXGI_SHARED_RESOURCE_WRITE.0, &name)
                    .context("CreateSharedHandle (preview)")?
            };
            let mutex: IDXGIKeyedMutex = tex.cast()?;
            slots.push(Slot { rtv, mutex, handle, _tex: tex });
        }
        let size = std::mem::size_of::<PreviewMeta>();
        let (map, view) = unsafe {
            let map = CreateFileMappingW(INVALID_HANDLE_VALUE, None, PAGE_READWRITE, 0, size as u32, &HSTRING::from(PREVIEW_META_NAME))
                .context("CreateFileMapping (preview meta)")?;
            let view = MapViewOfFile(map, FILE_MAP_ALL_ACCESS, 0, 0, size);
            anyhow::ensure!(!view.Value.is_null(), "MapViewOfFile");
            (map, view)
        };
        let _ = GENERIC_ALL;
        let ring = Self { slots, map, view, next: 0, width, height, skipped: 0, frame: 0 };
        let m = ring.meta();
        m.width.store(width, Ordering::Relaxed);
        m.height.store(height, Ordering::Relaxed);
        m.adapter_luid_low.store(luid.LowPart, Ordering::Relaxed);
        m.adapter_luid_high.store(luid.HighPart, Ordering::Relaxed);
        m.generation.fetch_add(1, Ordering::Release);
        Ok(ring)
    }

    fn meta(&self) -> &PreviewMeta {
        unsafe { &*(self.view.Value as *const PreviewMeta) }
    }

    /// Downscale `canvas` into the next free slot. Never blocks.
    pub fn publish(
        &mut self,
        ctx: &ID3D11DeviceContext,
        quad: &QuadPipeline,
        canvas: &ID3D11ShaderResourceView,
        canvas_w: u32,
        canvas_h: u32,
    ) -> Result<()> {
        self.frame += 1;
        let i = self.next;
        self.next = (self.next + 1) % self.slots.len();
        let slot = &self.slots[i];
        if !try_acquire(&slot.mutex, 0) && !try_acquire(&slot.mutex, 1) {
            self.skipped += 1;
            return Ok(());
        }
        let outer = Rect { x: 0.0, y: 0.0, w: self.width as f32, h: self.height as f32 };
        let dst = Rect::fit(canvas_w as f32, canvas_h as f32, outer);
        quad.begin(ctx, &slot.rtv, self.width, self.height, [0.0, 0.0, 0.0, 1.0]);
        let r = quad.draw(ctx, canvas, dst, self.width, self.height, 1.0, true);
        unsafe {
            ctx.OMSetRenderTargets(None, None);
            let _ = slot.mutex.ReleaseSync(1);
        }
        r?;
        let m = self.meta();
        m.seq.fetch_add(1, Ordering::AcqRel);
        m.latest_slot.store(i as u32, Ordering::Relaxed);
        m.frame_number.store(self.frame, Ordering::Relaxed);
        m.seq.fetch_add(1, Ordering::AcqRel);
        Ok(())
    }
}

impl Drop for PreviewRing {
    fn drop(&mut self) {
        unsafe {
            let _ = UnmapViewOfFile(self.view);
            let _ = CloseHandle(self.map);
            for s in &self.slots {
                let _ = CloseHandle(s.handle);
            }
        }
    }
}
