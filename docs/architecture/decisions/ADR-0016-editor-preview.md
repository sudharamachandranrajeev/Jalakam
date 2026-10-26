# ADR-0016: Editor preview: HTML video for playback, worker-decoded exact frames when paused

- **Status:** Accepted, pending POC-4
- **Related:** PRD TL-008, ADR-0003, ADR-0013

## Context
The clip editor needs smooth playback of long recordings and frame-exact positioning for cuts.

HTML `<video>` in WebView2 has two limits:
- Its `currentTime` seek is not frame-exact.
- It cannot play MKV.

It does play H.264/AAC MP4 well.

## Options
1. **Hybrid:** `<video>` plays a proxy MP4 or the remuxed source. On pause or scrub release:
   1. The UI requests the exact frame from the render worker (FFmpeg decode by timestamp).
   2. The UI shows that frame as an image over the video element until playback resumes.
2. **Native decode + preview swapchain** for the editor, as in the live preview. Frame-exact, but more native code, and the airspace rule then applies to the editor too.
3. **WebCodecs in WebView2:** promising, but container demuxing must be done in JS/WASM. Less mature.

## Decision
Option 1 for release one.
- MKV recordings are remuxed to MP4 for editing; they are not re-encoded.
- Cut points always come from the EDL's rational frame times, never from the video element's time.

## Consequences
- During playback, the displayed frame can be off by one frame. The paused frame and the export are exact.
- Decoding runs in a worker, which isolates crashes from untrusted files.

## Rejected
- **Option 2 for release one:** higher cost. Revisit if POC-4 fails.
- **Option 3:** not mature enough.

## Validation (POC-4, optional)
Seek to 100 random frame positions in a 4 h recording. At each position:
- the paused frame must match the export's frame (L2 hash);
- the seek must finish within the 150 ms budget.
