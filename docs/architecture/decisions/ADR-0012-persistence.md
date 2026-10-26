# ADR-0012: Persistence: SQLite (WAL) for records, atomic JSON documents for user documents

- **Status:** Accepted
- **Related:** PRD SYS-001/002/010, component-boundaries.md §10

## Context
There are two kinds of data:
- **Records:** library index, jobs and publish records. They are queryable and have many rows.
- **Documents:** scene collections, clip projects and preferences. They are human-readable, exportable, versioned and recoverable.

## Decision
- **Records:** SQLite via `rusqlite`.
  - WAL mode, with one writer connection in the App Host.
  - `synchronous=NORMAL`; `FULL` for publish records.
  - Migrations are numbered and run at start, after a snapshot.
- **Documents:** JSON with a `schema` version field.
  - Save sequence: write a temp file → `FlushFileBuffers` → `MoveFileExW(MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH)`.
  - The last 10 versions are kept in `.history/`.
  - Autosave is debounced to ≤ 1 write per 2 s.
- **Migrations:** pure functions `vN → vN+1` in `jk-model`, with golden-file tests.
  - Before migrating, the whole data folder is archived.
  - If a migration fails, the archive is restored and the app opens read-only.
- **Recordings** carry a sidecar event log (markers, scene changes), so the library can be rebuilt from disk.

## Rejected
- **Everything in SQLite:** documents become opaque and hard to share or diff.
- **Everything in JSON:** library queries over thousands of items are slow, and partial writes corrupt indexes.
- **sled, redb or other embedded KV stores:** less tooling, and no SQL for diagnostics.

## Consequences
There are two storage mechanisms to test. Crash-safety tests kill the App Host during writes (FI tests).

## Validation
A power-cut simulation runs in a loop: write, kill at a random point, reload. Expected result: the last or the previous version, never a corrupt file.
