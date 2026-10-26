# ADR 043 — explicit cell geometry and source-preserving ink reflow

Status: implementation/component verification in progress; complete Q071 and backend acceptance remain open.

A table can carry optional source-unit geometry: a named coordinate space, top-left origin, one height per row and one width per column. Dimensions must be finite and positive, fit the logical table and produce finite bounds. Existing tables without geometry retain their serialized shape and cell-ink interpretation.

Each retained cell-region stroke has an immutable capture/adoption cell origin. New capture into a laid-out table requires the same named source space and records the current cell origin. Legacy ink adoption requires explicit origins for every region stroke; the backend refuses incomplete/foreign-space mappings. Subsequent layout edits retain those original origins and source-space identity. No grid detection, physical calibration, clipping or inferred mapping is introduced.

Resizing changes cell bounds; it does not scale handwriting. Inserting/removing a row or column translates neighbouring cells and their ink. Sized insertion supplies an explicit height/width rather than guessing from a device or neighbour. Deleting a cell hides its current strokes and removes its active region/mapping, while retaining original observations and source membership independently of Undo. Existing revision-bound transactions retain the complete operation for Undo/Redo.

A table ink scene pairs unchanged original strokes with derived source-to-surface transforms, composing cell reflow before the block transform. Selected-cell recognition uses derived cell-local ordered vectors so contacts captured before/after reflow align; its source fingerprint binds the original observations and exact block/layout. Persisted points, style, source records and receipt fingerprints are never rewritten.

Native subset v13 and SQLite/library backup schema21 fence the new geometry in current blocks, retained history and Undo payloads. No physical SQL column or old payload rewrite is needed. Native independent copy remaps cell and source references within origins. Both encrypted restore formats validate source-space/mapping semantics and reject old headers carrying the new feature.

Full scene/export/H.I. integration, cell move/copy operations, remaining malformed/resource/protection coverage and final native/Android validation are still required. No real-device placement or recogniser-quality claim follows from synthetic source-unit tests.
