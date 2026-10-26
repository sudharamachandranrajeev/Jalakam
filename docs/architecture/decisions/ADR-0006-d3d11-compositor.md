# ADR-0006: Direct3D 11 compositor on a single render thread

- **Status:** Accepted
- **Related:** ADR 0001 D1, technology-evaluation.md E7

## Context
The compositor renders two canvases at up to 60 fps. It applies transforms, crops, colour conversion and simple effects.

It must hand textures to NVENC, AMF and QSV without CPU copies. It must also interoperate with:
- WGC;
- Media Foundation;
- CEF shared textures;
- the preview's shared texture.

## Options
1. **D3D11** (feature level 11_0+), with a single immediate context on one render thread.
2. **D3D12:** explicit control, but more work for little gain at this load. FFmpeg's D3D12 hwframes path is newer.
3. **wgpu:** portable, but there is no D3D11 hwframes interop for FFmpeg. Reaching WGC or shared handles needs unsafe escapes into the HAL.
4. **Vulkan:** no WGC interop. Every Windows capture API would need an interop layer.

## Decision
D3D11. One device is shared by capture (WGC frame pool, MF DXGI device manager), the compositor and the encoders.

The render thread owns the device and is the only thread that issues draws. Other threads touch the device only where a Windows API requires it (MF), and then only through the device's multithread protection.

## Consequences
- **Zero-copy:** every Windows capture API and FFmpeg hwframes (`AV_HWDEVICE_TYPE_D3D11VA`) speak D3D11 natively, so zero-copy is possible end to end.
- **Simple synchronisation:** one context.
- **Feature ceiling:** no mesh shaders or explicit multi-queue. Release one does not need them.
- **TDR handling:** on device removed (TDR), recreate the device and its resources (component-boundaries.md §4).

## Rejected
- **D3D12:** cost without benefit for the MVP. Revisit for HDR or AV1 multi-session work.
- **wgpu and Vulkan:** the interop gaps above.

## Validation
POC-1A checks two things:
- compositor GPU time against performance-budgets.md §1;
- recovery from a forced TDR (`dxcap -forcetdr`).
