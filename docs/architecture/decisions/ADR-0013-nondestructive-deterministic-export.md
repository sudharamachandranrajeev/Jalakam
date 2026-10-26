# ADR-0013: Non-destructive editing and deterministic export manifests

- **Status:** Accepted
- **Related:** PRD TL-*, EXP-*, component-boundaries.md §6, §8

## Context
Clips are cut from multi-hour recordings. Users expect two things:
- originals never change;
- the same project exports the same way every time.

## Decision
- **Non-destructive editing:**
  - Every component opens source media read-only.
  - Projects are edit decision lists (EDLs). They reference media by library ID + content hash.
  - Variants (landscape, portrait) are views over the same EDL, each with its own reframing.
  - Exports always go to new files.
- **Render manifest:** each export job pins:
  - the project revision and media hashes;
  - the preset version and the FFmpeg build;
  - the encoder and driver versions;
  - every effect parameter.

  The manifest is stored next to the output.
- **Determinism levels:**
  - **L1, structural:** same duration, streams, frame count and cut points (frame-accurate) on every machine. Guaranteed.
  - **L2, pre-encode frames:** identical decoded and composited frames on the same software build. Checked by hashing each frame before encoding. Guaranteed for the CPU render path; GPU paths are tested per driver.
  - **L3, bit-identical files:** only for the same machine, driver and encoder settings. Best effort; not promised to users.
- **Time representation:** frame times are rational (`num/den`), never floating-point seconds.

## Rejected
- **Destructive trimming of recordings to save space:** irreversible, and it conflicts with re-editing.
- **Promising bit-identical exports across machines:** impossible with hardware encoders and driver updates.

## Consequences
Disk usage stays high because originals are kept. The library shows space use and offers explicit deletion of originals, with user confirmation.

## Validation
A determinism test exports the same project twice and compares the L1 and L2 hashes. It runs in CI on the CPU path.
