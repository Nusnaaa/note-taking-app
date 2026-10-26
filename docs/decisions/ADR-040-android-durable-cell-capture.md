# ADR 040 — Android durable table-cell capture

Status: backend transport/recovery implementation; component and full validation remain separate. No production H.I.

A CaptureCell binds an existing table block, cell and expected document revision. A capture session can select a cell or a canvas layer, never both. The active journal's STEMAC06 record retains that binding independently of observed points, frame revision, tool/style and source space. Readers retain versions 01–05 with no invented cell target. Prefix extension cannot change the binding. Completed/interrupted records that encounter stale, hidden, missing or denied destinations stay in private recovery storage rather than falling back to a new freehand block.

CaptureRequest sends the existing CaptureCell authoring command with the journal's entity. The Core/Host route adds an optional expected entity and verifies it inside the capture transaction, preserving the older direct API while preventing Android routing metadata from mislabelling a valid foreign table. Exact target/payload receipt checks remain authoritative; an entity-level receipt alone never confirms arbitrary replacement points.

The writer advances a live cell session only after a native response identifies the expected block/entity and its exact next revision. It does not rebase over an unrelated edit. Each caller Submission owns a separate nonserialisable retry binding, minted for its session/ticket and frozen on first admission. The session's bounded lookup keeps weak references and retires entries after actual commit/cancel/disposal. Callers can therefore retain an old exact retry without making every completed contact occupy pending admission capacity. Recoverable prefixes can reconstruct their binding from the durable journal if caller frames have gone away. Cancelling a future remains distinct from retiring an admitted write.

Recovery exposes cell identity and destination availability separately from layer availability. Explicit same-entity cell retargeting requires a current source checksum, sealed uncommitted evidence, no conflicting legacy outbox entry and a freshly authorised existing destination cell at its exact revision. Confirmation repeats those checks before changing private routing; a changed destination rejects the confirmation. The private checkpoint acknowledgement is separate from SQLite publication. Points, style, order, IDs, library/entity and source space remain unchanged. Layer retargeting refuses cell captures. Cross-entity recovery and physical cell hit-testing/layout are separate work.

The process-exit journal fixture now retains a cell target through file-sync, rename and directory-sync crashes. Legacy v3 and v5 fixtures use their actual historical layouts; merely changing a new-format tag is not a legacy fixture. The initial misplaced test-method compilation failure and the initial synthetic-v3-layout failure are retained. Component cases exercise sequential native cell captures, exact retry, typed recognition/explicit acceptance with untouched neighbours, stale recovery/reopen, stale-confirmation fencing, wrong-entity refusal and more than 256 completed contacts while an old retry binding remains live. These are synthetic observations with actual desktop JNI/API35 simulation, not handwriting quality, device performance or complete backend acceptance.

The underlying ADR039 global-region/cell-ID alias still needs a compatibility
follow-up: earlier table values allow IDs local to each table, including repeated
IDs across different tables. Such legacy values must remain independently
capturable. Decoupling global ink-region identity from local cell targeting is
required before full table acceptance; this transport milestone does not close it.
