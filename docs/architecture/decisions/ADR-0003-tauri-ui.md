# ADR-0003: Tauri 2 + React UI with a native preview surface; Flutter as fallback

- **Status:** Accepted, conditional on POC-1C
- **Related:** ADR 0001 D3, technology-evaluation.md E2, ADR-0008

## Context
The UI is a dense editor (scenes, mixer, library, timeline, AI review) plus a 60 fps preview of one or two canvases. The preview must not depend on browser compositing, and the UI must never block the engine (NFR-REL-03).

## Options
1. **Tauri 2 + React/TypeScript**, with the preview in a native child HWND that has its own D3D11 swapchain.
2. **Flutter for Windows**, with the preview through a texture registrar or a platform view.
3. **Qt 6 / QML**, with the preview as a native QQuickItem.
4. **Electron:** rejected. It ships a full Chromium bundle on top of CEF and offers nothing over WebView2.
5. **Pure Win32 / WinUI 3:** rejected. UI iteration is slower. WinUI 3 interop with D3D11 swap chains works, but its packaging and the C++/C# split add complexity.

## Decision
Tauri 2 + React.
1. The App Host creates a child HWND over the preview area.
2. It receives the engine's preview texture through a shared DXGI NT handle with a keyed mutex.
3. It copies the texture into its own flip-model swapchain and presents it.

**Airspace rule:** React draws all chrome around the preview, never over it.

Selection handles, guides and safe areas are drawn natively in the preview swapchain. The UI sends a small overlay description for them.

## Consequences
- **Gains:** fast UI iteration, a strong component ecosystem for the editor, and a small footprint, because WebView2 ships with Windows 11.
- **Design constraint:** the airspace rule means no HTML tooltips or menus over the preview. Context menus are native or are placed outside the preview.
- **Integration cost:** there are two rendering stacks, WebView2 and the preview swapchain. Resize, DPI and focus handling across them needs care.

## Rejected
- **Qt/QML (E2: 37):** it has the most mature native-preview story. LGPL obligations are manageable under Apache-2.0. The problem is team complexity from a C++/QML/Rust tri-language stack.
- **Flutter (E2: 40 vs 42 for Tauri):** performance is good and the Dart UI has one toolchain. However:
  - Windows texture interop for D3D11 shared textures is less proven.
  - The Rust bridge adds a third language.

  **Flutter is kept as the fallback** if POC-1C fails.

## Validation (POC-1C)
POC-1C passes only if all of these hold:
- 60 fps preview in a Tauri window with no tearing;
- ≤ 1 frame of added latency;
- correct behaviour on resize;
- correct behaviour when the window moves between monitors at 100 % and 150 % DPI;
- correct behaviour on minimise and restore;
- keyboard focus moves correctly between React and the preview;
- no UI jank while the preview runs.

If any check still fails after 1 week, switch to Flutter and re-run the POC.
