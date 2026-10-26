//! Text overlay drawn with Direct2D/DirectWrite into a D3D11 texture.

use anyhow::Result;
use windows::core::{w, Interface};
use windows::Win32::Graphics::Direct2D::Common::*;
use windows::Win32::Graphics::Direct2D::*;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::DirectWrite::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::Graphics::Dxgi::IDXGISurface;

use crate::gpu;

pub struct Overlay {
    pub texture: ID3D11Texture2D,
    pub srv: ID3D11ShaderResourceView,
    pub width: u32,
    pub height: u32,
    rt: ID2D1RenderTarget,
    format: IDWriteTextFormat,
    brush: ID2D1SolidColorBrush,
    bg: ID2D1SolidColorBrush,
}

impl Overlay {
    pub fn new(dev: &ID3D11Device, width: u32, height: u32) -> Result<Self> {
        let texture = gpu::create_texture(
            dev,
            &gpu::texture_desc(
                width,
                height,
                DXGI_FORMAT_B8G8R8A8_UNORM,
                D3D11_BIND_FLAG(D3D11_BIND_RENDER_TARGET.0 | D3D11_BIND_SHADER_RESOURCE.0),
                D3D11_RESOURCE_MISC_FLAG(0),
            ),
        )?;
        let srv = gpu::create_srv(dev, &texture)?;
        unsafe {
            let factory: ID2D1Factory = D2D1CreateFactory(D2D1_FACTORY_TYPE_MULTI_THREADED, None)?;
            let surface: IDXGISurface = texture.cast()?;
            let props = D2D1_RENDER_TARGET_PROPERTIES {
                r#type: D2D1_RENDER_TARGET_TYPE_DEFAULT,
                pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED },
                dpiX: 96.0,
                dpiY: 96.0,
                ..Default::default()
            };
            let rt = factory.CreateDxgiSurfaceRenderTarget(&surface, &props)?;
            let dw: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
            let format = dw.CreateTextFormat(
                w!("Consolas"),
                None,
                DWRITE_FONT_WEIGHT_BOLD,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                28.0,
                w!("en-us"),
            )?;
            let brush = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 1.0 }, None)?;
            let bg = rt.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.0, g: 0.0, b: 0.0, a: 0.6 }, None)?;
            Ok(Self { texture, srv, width, height, rt, format, brush, bg })
        }
    }

    pub fn draw(&self, text: &str) -> Result<()> {
        let wide: Vec<u16> = text.encode_utf16().collect();
        let rect = D2D_RECT_F { left: 0.0, top: 0.0, right: self.width as f32, bottom: self.height as f32 };
        unsafe {
            self.rt.BeginDraw();
            self.rt.Clear(Some(&D2D1_COLOR_F { r: 0.0, g: 0.0, b: 0.0, a: 0.0 }));
            self.rt.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect, radiusX: 10.0, radiusY: 10.0 }, &self.bg);
            let inner = D2D_RECT_F { left: 14.0, top: 8.0, right: rect.right - 10.0, bottom: rect.bottom - 6.0 };
            self.rt.DrawText(&wide, &self.format, &inner, &self.brush, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);
            self.rt.EndDraw(None, None)?;
        }
        Ok(())
    }
}
