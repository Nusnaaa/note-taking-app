# ADR-004: capture, technical objects, local processing and library recovery

Status: implemented partial core increment, 22 October 2026. Extends ADR-001–003;
no change to approved v0.5.8, backend-first sequencing or personal-use scope.

## Capture and storage boundary

The portable input router accepts classified **observed** points. Predicted display
points have no input field. Palm events are ignored, touch defaults to navigation,
and a newly detected stylus cancels tentative finger ink. Committed source samples
remain immutable. The Android adapter must supply real event classification and
cancellation; it is not implemented by this portable contract.

The bounded queue never silently discards accepted work. Rejected admission leaves
ownership with the caller; failed persistence leaves the front item available for
retry. A capture ticket and content fingerprint commit in the same SQLite transaction
as its block, source strokes and exercise membership. Repeating an acknowledged or
uncertain ticket is idempotent; reusing it with changed content fails. Memory queue
contents are not durable before that transaction commits.

Background admission uses explicit byte reservations and cooperative cancellation.
Capture cancels background controls and prevents new admission, while a running
worker retains its reservation until its lease ends. Cancellation cannot forcibly
stop an in-process native worker. A reserve-file primitive writes and syncs real
bytes and releases only its owned file. Android scheduling, available-space detection,
emergency flushing and physical power-loss behaviour remain separate obligations.

## Editable technical data and views

Tables retain cell identities and literal values; diagrams retain component geometry
and labels; graphs retain authored axes/units/ordered series without fitting or
solving. Image bytes have independent immutable IDs and SHA-256 integrity. Import
checks PNG/JPEG signatures and size, not successful decoding. Decoders remain host
work. Group transforms update all members atomically and preserve original strokes.

Layout is explicit. Linked canvas embeds share one source and keep independent
camera/frame/pin state; recursive embed cycles are rejected. Reading an embed cannot
bypass source-block protection. Removing an embed preserves its source. Persisted
working/tool views retain camera, pen options and block anchors. Protected surface
placeholders disclose presence without content or titles. Layers, overlays, flow,
follow-object cameras, caret/selection persistence and general operation undo remain
open; these primitives do not constitute the whole canvas feature set.

## Replaceable local engines

Consent precedes local resource staging. Bounded streaming verifies a digest before
metadata publication; the core neither downloads resources nor loads arbitrary
plugin code. Descriptor evidence records an eligibility declaration, not an automated
legal opinion. No proprietary model is bundled and synthetic fixtures claim no accuracy.

Activation checks resource integrity and probes a provisional compiled adapter.
Generation checks reject intervening writes before committing the selected version.
Only after successful commit does an infallible runtime swap replace the old adapter.
Failed probe/transaction preserves the old version; an earlier installed version can
be reactivated. Defaults are durable, but every new process must establish runtime
readiness again. Removal disables selection and preserves notebooks and accepted
work. Resource files await explicit lease-aware cleanup. Converter lifecycle, hard
worker isolation and actual recogniser integration remain open.

## Sources, portability and derivatives

PDF revision replacement is reviewed and generation-checked. Old pinned anchors
remain historical; live anchors need explicit reconnection. Source cache revocation
cannot delete independently authorised snapshots or authored annotations. Reacquiring
a revoked source requires a new host grant and the exact expected digest. The core
never treats a filename as identity or retrieves a remote file.

Native subset v3 extends v2 with rich attachments and independent ID remapping. It
still refuses source-linked documents and omits general library state. A distinct
whole-library logical snapshot includes all current schema-8 tables, retained history
and Trash, encrypted using the existing password/recovery envelope. Restore creates
a separate library and never overwrites a live database. Provider grants and model
resources are not portable. See the format documents for source-inclusion controls,
limits and validation; this is not an independent cryptographic audit.

Converted-only Markdown/SVG output omits original ink unless explicitly selected,
requires fresh authentication for protected content and rejects stale reviews. Crop
boundaries fail closed where a safe derivative is not implemented. Pagination accepts
measured indivisible object bounds and never silently shrinks or bisects them. Actual
PDF writing, Unicode/maths layout, complete diagram/graph presentation, static embeds
and image cropping remain open. Current vector output preserves paths and title
metadata; it does not claim complete visible axes, labels or layout fidelity.

All numerical resource ceilings are defensive implementation limits, not approved
performance SLOs. No production H.I., network inference, telemetry, automatic cloud
backup or real-device/manual result is introduced by this decision.

## Whole-library validation continuation — 25 October 2026

Whole-library restore uses SQLite-backed semantic validation independently of the
native subset format. Restore stages retain the normal notebook schema/journaling;
private validation tables/indexes are created under a savepoint and always rolled
back. This avoids unbounded Rust graph collections and SQLite TEMP placement
outside the private stage without changing healthy-library transaction policy.
Embed-cycle traversal uses an indexed SQLite stack and colours instead of native
recursion or an in-memory whole graph. All former native-subset invariants remain,
with explicit rejection of raw stroke rows absent from block source membership.
The former aggregate path is compiled only for differential tests. Incremental
SQLite BLOB hashing now covers images as well as retained source assets. No
content is repaired or discarded to fit subset export limits. Per-record/input
limits and a total native staging allocation budget remain separate obligations.
