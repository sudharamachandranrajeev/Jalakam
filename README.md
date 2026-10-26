# Jalakam

Jalakam is a desktop streaming studio with AI-assisted highlight detection, transcript-based clip editing, dual landscape/portrait canvases, multistreaming and clip publishing.

**Status:** Phase 0, research only. No application code yet.

## Clean-room implementation

Jalakam is an independent, clean-room implementation. Phase 0 studied the publicly documented behaviour of existing streaming tools, including Meld Studio and OBS Studio, using public documentation and non-invasive inspection of a legitimately installed copy. Only functional findings were recorded.

- No third-party source code, decompiled code, UI assets or configuration data are included or used.
- Implementation works from the requirements in `docs/research/` and public standards (RTMP / Enhanced RTMP, H.264/HEVC, platform APIs).
- "Meld Studio" and "OBS Studio" are trademarks of their respective owners. Jalakam is not affiliated with either.

## Research pack

- [Product landscape](docs/research/product-landscape.md)
- [Reference workflows](docs/research/reference-workflows.md)
- [Feature matrix](docs/research/feature-matrix.md)
- [Unknowns and risks](docs/research/unknowns-and-risks.md)
- [MVP recommendation, build-model plan and pending decisions](docs/research/mvp-recommendation.md)
- [Source register](docs/research/source-register.md)

## Licence

Not yet chosen. This is pending decision D2 in the MVP recommendation. Until a licence is added, all rights are reserved.
