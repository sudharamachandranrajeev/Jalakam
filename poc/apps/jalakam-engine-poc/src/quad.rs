//! Textured-quad pipeline used by the compositor and the preview downscale.

use anyhow::{anyhow, Result};
use windows::core::{s, PCSTR};
use windows::Win32::Graphics::Direct3D::Fxc::D3DCompile;
use windows::Win32::Graphics::Direct3D::*;
use windows::Win32::Graphics::Direct3D11::*;

const HLSL: &str = r#"
cbuffer Q : register(b0) { float4 dst; float4 src; float4 params; };
struct VSOut { float4 pos : SV_Position; float2 uv : TEXCOORD0; };
VSOut vs_main(uint id : SV_VertexID) {
    float2 t = float2(id & 1, id >> 1);
    VSOut o;
    o.pos = float4(lerp(dst.x, dst.z, t.x), lerp(dst.y, dst.w, t.y), 0, 1);
    o.uv = float2(lerp(src.x, src.z, t.x), lerp(src.y, src.w, t.y));
    return o;
}
Texture2D tex : register(t0);
SamplerState smp : register(s0);
float4 ps_main(VSOut i) : SV_Target {
    float4 c = tex.Sample(smp, i.uv);
    // params.y = 1 forces opaque alpha (captured surfaces carry garbage alpha)
    c.a = lerp(c.a, 1.0, params.y);
    return float4(c.rgb, c.a * params.x);
}
"#;

#[repr(C)]
#[derive(Clone, Copy)]
struct Constants {
    dst: [f32; 4],
    src: [f32; 4],
    params: [f32; 4],
}

pub struct QuadPipeline {
    vs: ID3D11VertexShader,
    ps: ID3D11PixelShader,
    cb: ID3D11Buffer,
    sampler: ID3D11SamplerState,
    blend: ID3D11BlendState,
}

/// Pixel rectangle inside a render target.
#[derive(Debug, Clone, Copy)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    /// Largest rectangle with the source aspect ratio centred inside `outer` (letterbox).
    pub fn fit(src_w: f32, src_h: f32, outer: Rect) -> Rect {
        if src_w <= 0.0 || src_h <= 0.0 {
            return outer;
        }
        let scale = (outer.w / src_w).min(outer.h / src_h);
        let (w, h) = (src_w * scale, src_h * scale);
        Rect { x: outer.x + (outer.w - w) / 2.0, y: outer.y + (outer.h - h) / 2.0, w, h }
    }
}

fn compile(entry: PCSTR, target: PCSTR) -> Result<Vec<u8>> {
    unsafe {
        let mut blob = None;
        let mut errors = None;
        let r = D3DCompile(
            HLSL.as_ptr() as *const _,
            HLSL.len(),
            None,
            None,
            None,
            entry,
            target,
            0,
            0,
            &mut blob,
            Some(&mut errors),
        );
        if let Err(e) = r {
            let msg = errors
                .map(|b: ID3DBlob| {
                    let p = std::slice::from_raw_parts(b.GetBufferPointer() as *const u8, b.GetBufferSize());
                    String::from_utf8_lossy(p).into_owned()
                })
                .unwrap_or_default();
            return Err(anyhow!("D3DCompile failed: {e} {msg}"));
        }
        let b: ID3DBlob = blob.unwrap();
        Ok(std::slice::from_raw_parts(b.GetBufferPointer() as *const u8, b.GetBufferSize()).to_vec())
    }
}

impl QuadPipeline {
    pub fn new(dev: &ID3D11Device) -> Result<Self> {
        let vsb = compile(s!("vs_main"), s!("vs_5_0"))?;
        let psb = compile(s!("ps_main"), s!("ps_5_0"))?;
        unsafe {
            let mut vs = None;
            dev.CreateVertexShader(&vsb, None, Some(&mut vs))?;
            let mut ps = None;
            dev.CreatePixelShader(&psb, None, Some(&mut ps))?;
            let mut cb = None;
            dev.CreateBuffer(
                &D3D11_BUFFER_DESC {
                    ByteWidth: std::mem::size_of::<Constants>() as u32,
                    Usage: D3D11_USAGE_DYNAMIC,
                    BindFlags: D3D11_BIND_CONSTANT_BUFFER.0 as u32,
                    CPUAccessFlags: D3D11_CPU_ACCESS_WRITE.0 as u32,
                    ..Default::default()
                },
                None,
                Some(&mut cb),
            )?;
            let mut sampler = None;
            dev.CreateSamplerState(
                &D3D11_SAMPLER_DESC {
                    Filter: D3D11_FILTER_MIN_MAG_MIP_LINEAR,
                    AddressU: D3D11_TEXTURE_ADDRESS_CLAMP,
                    AddressV: D3D11_TEXTURE_ADDRESS_CLAMP,
                    AddressW: D3D11_TEXTURE_ADDRESS_CLAMP,
                    MaxLOD: f32::MAX,
                    ..Default::default()
                },
                Some(&mut sampler),
            )?;
            let mut bd = D3D11_BLEND_DESC::default();
            bd.RenderTarget[0] = D3D11_RENDER_TARGET_BLEND_DESC {
                BlendEnable: true.into(),
                SrcBlend: D3D11_BLEND_SRC_ALPHA,
                DestBlend: D3D11_BLEND_INV_SRC_ALPHA,
                BlendOp: D3D11_BLEND_OP_ADD,
                SrcBlendAlpha: D3D11_BLEND_ONE,
                DestBlendAlpha: D3D11_BLEND_INV_SRC_ALPHA,
                BlendOpAlpha: D3D11_BLEND_OP_ADD,
                RenderTargetWriteMask: D3D11_COLOR_WRITE_ENABLE_ALL.0 as u8,
            };
            let mut blend = None;
            dev.CreateBlendState(&bd, Some(&mut blend))?;
            Ok(Self {
                vs: vs.unwrap(),
                ps: ps.unwrap(),
                cb: cb.unwrap(),
                sampler: sampler.unwrap(),
                blend: blend.unwrap(),
            })
        }
    }

    /// Bind a render target of `rt_w` x `rt_h` pixels and set pipeline state.
    pub fn begin(&self, ctx: &ID3D11DeviceContext, rtv: &ID3D11RenderTargetView, rt_w: u32, rt_h: u32, clear: [f32; 4]) {
        unsafe {
            ctx.OMSetRenderTargets(Some(&[Some(rtv.clone())]), None);
            ctx.ClearRenderTargetView(rtv, &clear);
            ctx.RSSetViewports(Some(&[D3D11_VIEWPORT {
                TopLeftX: 0.0,
                TopLeftY: 0.0,
                Width: rt_w as f32,
                Height: rt_h as f32,
                MinDepth: 0.0,
                MaxDepth: 1.0,
            }]));
            ctx.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLESTRIP);
            ctx.IASetInputLayout(None);
            ctx.VSSetShader(&self.vs, None);
            ctx.PSSetShader(&self.ps, None);
            ctx.VSSetConstantBuffers(0, Some(&[Some(self.cb.clone())]));
            ctx.PSSetConstantBuffers(0, Some(&[Some(self.cb.clone())]));
            ctx.PSSetSamplers(0, Some(&[Some(self.sampler.clone())]));
            ctx.OMSetBlendState(&self.blend, None, 0xffff_ffff);
        }
    }

    /// Draw `srv` into pixel rect `dst` of a target sized `rt_w` x `rt_h`.
    pub fn draw(
        &self,
        ctx: &ID3D11DeviceContext,
        srv: &ID3D11ShaderResourceView,
        dst: Rect,
        rt_w: u32,
        rt_h: u32,
        opacity: f32,
        force_opaque: bool,
    ) -> Result<()> {
        let (w, h) = (rt_w as f32, rt_h as f32);
        let c = Constants {
            dst: [
                dst.x / w * 2.0 - 1.0,
                1.0 - dst.y / h * 2.0,
                (dst.x + dst.w) / w * 2.0 - 1.0,
                1.0 - (dst.y + dst.h) / h * 2.0,
            ],
            src: [0.0, 0.0, 1.0, 1.0],
            params: [opacity, if force_opaque { 1.0 } else { 0.0 }, 0.0, 0.0],
        };
        unsafe {
            let mut m = D3D11_MAPPED_SUBRESOURCE::default();
            ctx.Map(&self.cb, 0, D3D11_MAP_WRITE_DISCARD, 0, Some(&mut m))?;
            std::ptr::copy_nonoverlapping(&c as *const Constants as *const u8, m.pData as *mut u8, std::mem::size_of::<Constants>());
            ctx.Unmap(&self.cb, 0);
            ctx.PSSetShaderResources(0, Some(&[Some(srv.clone())]));
            ctx.Draw(4, 0);
            ctx.PSSetShaderResources(0, Some(&[None]));
        }
        Ok(())
    }
}
