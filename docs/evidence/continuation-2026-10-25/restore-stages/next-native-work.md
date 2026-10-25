# Next native restore work — inspected gaps, not implemented capability

The next target is bounded whole-library semantic validation and staging allocation.
Current source inspection establishes these concrete gaps:

- backup.rs::validate_backup_state collects payload tables into Vecs and then
  rebuilds an entire NativeDocument, including blocks, original strokes, histories
  and image bytes. Streaming ciphertext therefore does not bound validation heap.
- NativeDocument/Attachments validation imports subset limits (10,000 entities,
  100,000 blocks and 64 MiB aggregate image bytes) into whole-library restore.
  These are actual code limits, not measured device limits or a passing probe.
  A scalable whole-library validator must not silently narrow to subset scale.
- Streaming cell decoding still materializes complete blobs/text values. Current
  ciphertext/plaintext/free-space controls are not total native memory/disk quotas.

Required implementation direction:

1. Preserve the existing complete invariant set while validating through indexed
   SQLite lookups and bounded row iteration. A private validation identity/region
   index can live inside the staged SQLite database and be removed before review;
   it must never appear in a published library or weaken identity/history, source
   membership, containment/cycle, image, layer, embed, view/bookmark, proposal,
   source-retention, edit-journal or schema-compatibility checks.
2. Hash/check large stored blobs incrementally via SQLite-owned blob/substring
   reads, never raw FD access or entire-graph byte collection. Large inputs need
   meaningful explicit record/allocation bounds; do not reintroduce an arbitrary
   small whole-library cap or label an input bound a total RSS quota.
3. Evaluate scratch-only pager/journal configuration and an explicit native stage
   allocation budget. max_page_count alone does not cap WAL/journal/temp spill.
   Any changed journaling may apply only to private unpublishable scratch; healthy
   live SQLite persistence retains transactional recovery. Both current streaming
   and legacy restore, validation work/indexes and publication temp space need
   honest accounting and interruption/cleanup outcomes.
4. Add negative/differential semantic coverage plus real above-subset-limit
   whole-library cases. Keep synthetic host execution separate from device
   acceptance and preserve all existing restore/protection/ink/ownership tests.

Other original backend gaps remain open. This note is not completion evidence,
not a quantitative SLO, and not authorisation to begin production H.I.
