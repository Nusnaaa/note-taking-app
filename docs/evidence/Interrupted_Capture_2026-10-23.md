# Versioned interrupted-capture provenance — 23 October 2026

Source checkpoint: `1f0e03c`, branch `phase-a/backend`.

Backend acceptance remains NOT MET and production H.I. remains gated. This is the
source model/format foundation for active-prefix recovery, not an automatic durable
active-stroke journal or a complete capture/lifecycle implementation.

Full exact-source validation passed with matching clean source manifests:
**170 Rust tests**, **74 Android unit/Robolectric tests**, and **nine desktop JNI
smoke groups**, with no failed/ignored/skipped tests. Format, strict workspace
Clippy, release build, traceability, ARM64 compilation/ELF, Android lint, AAR native
payload and isolated-service manifest checks passed. Commands, hashes and logs are
in `continuation-2026-10-23/interrupted-capture/` and `latest/`.

## Delivered behavior

Source strokes distinguish completed observations, active/uncommitted prefixes and
interrupted observations with explicit Lifecycle/Recovery provenance. Legacy strokes
omit the new `end` field and keep completed semantics. Default serialization still
omits it, preserving old source payloads/digests and capture receipt fingerprints.
No pen-up point, coordinates or timestamp is fabricated when sealing a prefix.

Rust InputRouter and Android InkAccumulator expose immutable active snapshots with
stable begin-allocated identities/session order. Completion preserves identity;
interruption seals only observed points. Tentative platform/palm cancellation
retains its previous rejection behavior and is not guessed to be a lifecycle event.
Active pointers and aggregate points have explicit admission bounds. Rejected
batches leave previous observations unchanged. These bounds establish no physical
pen save-latency or zero-loss promise.

Active prefixes cannot enter persisted source ink, the normal capture encoder/queue
or a native document. Sealed interrupted strokes cross Android/JNI with provenance;
normal completion retains legacy wire encoding. MotionCapture exposes a lifecycle
interruption seam. Checkpoint snapshots themselves are never save acknowledgements.
The generic Kotlin DTO is CapturedStroke, with CompletedStroke retained as a source
alias for existing callers (no released binary-compatibility claim).

SQLite schema 10 versions the immutable source payload extension without rewriting
existing payloads. Native subset v5 carries interrupted provenance and reads v1-v4
completed sources. Old native version tags cannot hide interrupted semantics.
Encrypted logical backup schema 10 preserves provenance and reads schema-8/9 legacy
completed data without inventing termination or old undo history. Active/unknown or
downgraded containers fail closed. Original samples remain independent of ordinary
undo/history expiry; no recognition, transformation or correction algorithm changes.

## Executed fixture scope

New native fixtures exercise stable checkpoint identity/begin order, aggregate
bounds, no fabricated sample, immutable snapshots, rejection of active input,
idempotent sealed capture retry and changed-end retry rejection. Schema-9 migration
retains exact stored source payload/hash and original capture receipt. Native-v5
copy remaps identities while preserving observations/provenance; v1-v4 readers and
downgrade rejection are covered. Encrypted schema-10/legacy-9 restoration retains
source ink/receipts and rejects metadata disguised as an older schema. Existing
schema-8 and v1 migration fixtures were updated to assert the new current schema.

Android fixtures exercise accumulator/MotionEvent lifecycle interruption versus
platform cancellation, stable identity and encoder admission. An actual desktop-JNI
fixture captures one interrupted observed prefix, retries its ticket, reopens the
library and checks its exact reason, sample count, timestamps, negative zero,
full-precision doubles, measured zero and unavailable channel values. MotionEvents
are synthetic and establish no physical tablet event-delivery guarantee.

Still open: durable active-prefix journal, automatic lifecycle/process recovery and
integration with Android capture priority/admission/status; complete all-operation
history, rich-object fidelity, full portability and other backend groups. No early
production H.I., weights, handwriting upload, remote inference, automatic OS content
backup or origin push is introduced. `handover-do-not-commit/` remains ignored and
untracked.

The backend gate exits 2 for substantive open implementation groups and incomplete
canonical/inherited coverage only. No stale/mismatched/incomplete automated-evidence
reason remains.
