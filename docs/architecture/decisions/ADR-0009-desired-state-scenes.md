# ADR-0009: Desired-state scene model with revisioned reconciliation

- **Status:** Accepted
- **Related:** component-boundaries.md §2–§4, ADR-0005

## Context
The App Host owns documents. The engine owns devices and GPU resources. The engine must survive an App Host restart and must never be left in a half-applied state.

## Decision
The App Host sends the engine a **desired state**: canvases, scenes, sources and the outputs configuration.
- Each version carries a revision number that only increases.
- On attach the App Host sends a full snapshot; after that it sends patches.

The engine **reconciles** each revision:
1. It diffs desired state against actual state.
2. It opens or closes devices off the render thread.
3. It swaps the render graph atomically at a tick boundary.
4. It reports the applied revision and the status of each item.

The engine never edits the desired state.

Actions that are events rather than state (start stream, save replay, add marker) are separate idempotent RPCs.

## Consequences
- **Simple reattach:** the new App Host reads `engine.state()` (applied revision + live status) and resends its latest snapshot.
- **Host-side features:** undo/redo, autosave and collaboration-style features live entirely in the App Host.
- **Diff cost:** the engine needs a careful diff of source definitions, so that unrelated edits do not restart a device. Each source has a stable ID and a settings hash.

## Rejected
- **Imperative command stream** ("add layer", "move layer"): order-dependent, and hard to replay after a crash.
- **Engine-owned documents:** puts persistence and migration logic into the real-time process.

## Validation
Property tests: a random sequence of patches applied through reconcile must reach the same actual state as one snapshot of the final document.
