# Fixed layer capture and durable destination recovery — 23 October 2026

Source checkpoint: `f7d5674`, branch `phase-a/backend`.
Backend acceptance remains NOT MET and production H.I. remains gated.

Full validation passed against matching clean source manifests: **217 Rust tests**,
**113 Android tests**, ten desktop JNI smoke groups, format, strict Clippy, release,
traceability, ARM64/ELF, Android lint, AAR payload and manifest checks. No tests failed,
were ignored or skipped. Commands, hashes, timings and logs are archived in
`continuation-2026-10-23/layer-capture/` and `latest/`.

## Delivered behavior

WritingTarget now supports an explicit CanvasLayer destination with Base/Custom ID.
AppendTo/CaptureTo commit new blocks and custom-layer membership in one transactional
undo operation, advancing layer revision. Raw coordinates, source ownership, style,
ending and samples remain independent of this routing metadata. Current canvas and
whole-source authority apply; visibility grants no access. Missing custom layers
reject with review required rather than falling back to another layer/base. An exact
committed receipt retry returns its existing block under current owner/block authority
even after layer removal, without recreating membership. Changed target/ticket content
conflicts. Existing Entity/Exercise receipt fingerprint encodings remain unchanged.

Android capture sessions have an immutable optional CaptureLayer. Null preserves
legacy Entity routing. Selecting a different destination means interrupting the old
session and creating a new one; old submissions/retries cannot inherit a later target.
Normal admission, retry, startup replay and graceful close carry the fixed value.
STEMAC03 records it separately from immutable observations. STEMAC01/02 remain readable
as legacy Entity destinations without an invented layer. Frozen v1/v2 fixtures check
style/sample compatibility. Malformed UTF-16 coordinate spaces in the legacy encoder
now reject before UTF-8 replacement; valid legacy request encoding stays unchanged.

Recovery reviews expose the original layer and availability: true/false, or null when
current layer metadata cannot be read. Unknown metadata does not hide independently
authored draft evidence behind an unrelated current block lock: context/app/inherited
recovery authority still applies, while acceptance is blocked unless availability is
known true. Missing layers remain explicitly reviewable/discardable. Returning the same
stable layer identity can permit later replay; a different destination is never guessed.
Retired target review/discard retains its separate scoped proof requirement.

The committed representation already fits schema 14/native v8 layer membership and
source assets, so neither document format nor source-ink serialization changes.

## Executed fixtures and remaining scope

Three Rust fixtures check source/membership commit, undo/redo/reopen, source retention,
exact retry after layer removal, missing/wrong-kind/protected destination rejection and
transaction rollback. Existing layer/native/backup/history fixtures also reran. Android
journal fixtures add fixed v2 style compatibility and durable destination/rebinding
checks alongside the v1 golden fixture. Actual JNI/MotionEvent coverage captures into
a selected layer, retries after removal, retains blocked completed ink through restart,
reports missing/unknown destinations, reviews own draft under unrelated source-block
protection, rejects acceptance and explicitly discards only that recovery copy. The
final protected-metadata case was included in the forced full Android rerun.

Reviewed retargeting of pending drafts to a different layer, persistent pane/selection
configuration, custom nested-embed assignment, full scene/export fidelity and complete
recovery/resource scheduling remain open. The common receipt path also needs an explicit
privacy audit of protected-block errors for mismatched retry payloads, beyond successful
retry access checks; no whole-boundary closure is claimed here. No latency/device or
whole-backend acceptance claim follows from these component checks.

All eight gap groups remain OPEN. The gate exits 2 solely for those groups and
canonical/inherited coverage; no stale automated-evidence reason remains. The ledger
retains 94 requirements, 103 inherited clauses and 158 candidates without inferred PASS.
The handover folder is ignored, untouched and absent from tracked files/reachable path
history. Local commits only; nothing pushed. No production H.I., model, recogniser,
external inference or automatic OS backup change.
