# Jalakam

Jalakam is a desktop streaming studio with AI-assisted highlight detection, transcript-based clip editing, dual landscape/portrait canvases, multistreaming and clip publishing.

**Status:** Phase 2, architecture decided. No application code yet; the next step is the POC-1 spine spike.

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

## Product definition (Phase 1)

- [Product requirements](docs/product/product-requirements.md)
- [MVP scope, release-one definition of done, success criteria](docs/product/mvp-scope.md)
- [User journeys](docs/product/user-journeys.md)
- [Acceptance criteria and release gates](docs/product/acceptance-criteria.md)
- [Assumptions and decisions needed](docs/product/assumptions-and-decisions-needed.md)

## Architecture (Phase 2)

- [System design](docs/architecture/system-design.md): process boundaries, data flows, streaming pipeline, lifecycle
- [Component boundaries](docs/architecture/component-boundaries.md): interfaces and ownership
- [Technology evaluation](docs/architecture/technology-evaluation.md): compared options and rejected candidates
- [Threat model](docs/architecture/threat-model.md)
- [Performance budgets](docs/architecture/performance-budgets.md)
- [Architecture decision records, selected stack and POC plan](docs/architecture/decisions/README.md)

## Decisions

- [ADR 0001: Phase 0 product decisions](docs/decisions/0001-phase0-decisions.md)
- [ADR-0002 to ADR-0016: architecture decisions](docs/architecture/decisions/README.md)

## Licence

[Apache-2.0](LICENSE). See ADR 0001, decision D2.
