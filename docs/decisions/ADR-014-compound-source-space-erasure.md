# ADR 014 — compound source-space erasure within one master surface

Status: recorded engineering extension of ADR 013; no production H.I. acceptance.

A user operation may erase a selected section spanning several blocks. Independent per-block commands cannot provide the required compound Undo. Add `EraseGestures` with explicit per-target block, revision and source-space gesture. Targets must be distinct and share one master entity; linked exercises continue to expose those same block identities. Every target must be visible in original-ink presentation and currently authorised, even when its gesture misses.

Review all candidates before writing, with one shared segment-pair admission budget. If every gesture misses, return the unchanged blocks without changing library generation or Undo. Otherwise recheck the exact current authorised blocks inside one SQLite `write_for` transaction and persist all hits through one per-master Undo operation. Any late stale/protected/malformed/mixed-space/resource/storage failure preserves every target. Each hit block advances once; missed blocks retain their revisions. Source samples and capture receipts remain independent.

The single-block command delegates to this operation. Existing 4,096-point/per-block candidate limits and 1,000,000 pair budget remain; a compound request additionally admits at most 256 distinct targets. These are implementation admission bounds, not performance SLOs. No cross-master document transaction, duplicate-block gesture merging, nib-outline/partial erasure, implicit view/device coordinate mapping or production H.I. gesture acceptance is implied.
