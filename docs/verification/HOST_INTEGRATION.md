# Backend host integration and developer inspection

This is a tested backend increment, not the complete I0+I1 app. Production H.I. remains gated.

## Local inspection

Use Rust 1.90.0, a C compiler and Python 3. For PDF tests, install DejaVu Sans or set
`STEM_TEST_FONT` to an appropriate local font. Dependencies are pinned in Cargo.lock.

```sh
cargo fetch --locked
python3 tools/validate.py
cargo run -p stem-core --bin notebook_cli -- /absolute/path/developer-notebook.db
```

The CLI consumes one JSON request per line and emits one response. For example:

```json
{"version":1,"command":{"op":"Create","kind":"Note","parent":null,"title":"Synthetic test note"}}
{"version":1,"command":{"op":"List","parent":null}}
```

Use returned IDs and revisions in subsequent commands. `core/src/host.rs::Command`
is the authoritative typed protocol: organisation, blocks, source capture, edits,
regions, undo/redo, searches, exercise edits/navigation, table/diagram edits, layouts,
groups, embeds, saved views, backlinks, locks and status. `AppendTo`/`CaptureTo` accept
`{ "Exercise": "UUID" }` or `{ "Entity": "UUID" }` as the writing target. Unknown versions,
unknown fields and malformed inputs fail. A successful edit response acknowledges
SQLite commit. Retry an ambiguous capture using the same ticket and identical data.
A rejected/full request must remain in the caller's buffer; it was not acknowledged.
The native host now reserves separate count and encoded-payload-byte budgets for
pending authoring and optional reads (including the executing request). Authoring,
capture, undo and access changes retain FIFO order and run ahead of queued reads.
A running operation is not preempted. `Host::submit` returns an admission receipt;
`PendingResponse::wait` returns the actual response. Dropping the receipt does not
cancel admitted work. Closing drains all admitted work before the writer exits.
`Host::subscribe_status` offers pull-based, coalesced content-free admission status;
there are no writer callbacks or per-observer event queues. Attention is sticky after
an admitted authoring/authentication failure for that host lifetime; these counters
are not a replacement for per-ticket SQLite commit receipts. JNI clones a library
handle under the registry lock and releases that lock before waiting or calling a
trusted verifier, so separate libraries do not block each other's registry access.
Native `Host::open` claims SQLite-managed exclusive locking and an initial
transaction before migrations or authoring. Duplicate Host/JNI opens, including
symlink/hard-link aliases, fail without truncating the database. Independent
libraries remain independent. The worker retains this connection lease across
commits and through shutdown drain. SQLite owns all database descriptors and their
POSIX-lock lifecycle; no extra raw database FD is opened for ownership. Initialization
failure joins the failed writer before returning. The lower-level Store API remains
a component API; production panes share the host's authoritative Store.

`Host::close` closes admission and waits for actual drain even while Arc callers
still exist. Concurrent closes share the same join; repeated closes are safe.
A verifier must not close its own writer: reentrant close fails without self-join,
closes admission and grants no future access. JNI removes the opaque handle under
the registry lock, releases that lock, and waits for Host close. A blocked close
therefore does not block registry access to another library.

The Android backend now uses its own single writer with reserved authoring/query
count and input-byte budgets, including running actions. Capture, edits, authentication,
relock and unknown/new commands preserve FIFO; queued optional reads yield to them.
Read classification follows the native host, including PDF lease/read queries. A
running action is never preempted, and continuous writing may defer queued reads.
Default admission limits are 64 authoring actions/32 MiB and 16 queries/8 MiB.
These are implementation capacity limits, not approved performance SLOs. Rejection
throws before admission; callers retain observations and retry the same ticket.

String charges conservatively allow four bytes per UTF-16 code unit plus envelope
space. Typed vector frames charge fixed per-stroke metadata, all retained strings
and 128 bytes per point. These input charges bound queued/running logical input;
they do not bound whole-process RSS, caller-owned snapshots, native allocations,
returned content, trusted callback closures or total replay/storage work. The typed
capture encoder remains on the writer rather than allocating full JSON on input.
Authentication scope strings also obey the JNI 65,536-character input limit before
admission. Closing rejects new actions and drains both lanes, including cancelled
observation futures. Even shutdownNow deliberately drains instead of retracting ink.

`writerStatus()` returns a content-free immediate admission snapshot.
`observeWriterStatus().next(timeoutMillis)` provides pull-based coalescing from an
observer thread without writer callbacks or per-observer queues. Slow observers
read the latest counters/byte charges; authoring execution/native failures set sticky
attention for this writer lifetime. A denied admission is reported to that caller
and does not set sticky execution failure. Counts and termination are not recovery,
SQLite commit or full library-retirement receipts: inspect capture receipts,
`captureRecoveryState()` and `closeFullyAsync()` respectively. Dependent callbacks
on returned CompletableFutures can still run on the completing thread; callers must
keep them short or explicitly dispatch them to their own executor. This increment
does not establish physical pen latency or device resource acceptance.

The CLI has no authentication bypass. This optional developer harness does not replace
or require the owner's deferred handwriting/model tests on a complete runnable app.

## Revision-bound caret and selection bookmarks

Saved views now optionally retain an anchor/focus `SelectionBookmark`. Equal points
represent a caret; direction and cross-block endpoints are preserved. Every point
names a block and its exact revision, an optional recognition region, and a typed
target. Text/numeric/code and labels use Unicode scalar offsets (not byte, UTF-16
or grapheme indices). Maths targets name a local presentation-node ID: leaf offsets
count scalars, while composite offsets name boundaries in ordered children. No AST
interpretation, text normalization, solving or repair occurs. Typed targets also
cover table-cell text/maths, diagram labels, graph axis/unit/series labels, image
captions, whole-content boundaries and visible observed ink samples. Source ink
remains independently immutable; a bookmark contains references, never copied text.

`SaveView` requires current visible blocks, valid targets, matching revisions and
current view/exercise/block authority, plus the existing optimistic view revision.
A later content/protection revision or exercise reassignment does not silently clamp,
move or retarget the saved position. `RestoreView` returns settings plus `None`,
`Current` or `NeedsReview` bookmark state. Apply the bookmark only for `Current`.
`View` remains a stored-settings retrieval API, not permission to apply a bookmark
without checking current state/revisions. Fresh block/exercise protection still gates
all returned bookmark metadata and replacement, including clearing an old bookmark.
Saved positions in independent master/exercise views remain independent. Settings
writes do not modify content revisions or observations. Explicitly clearing or saving
a freshly reviewed current position replaces the bookmark transactionally.

Schema 11 adds optional bookmark payloads without rewriting existing saved views or
source data. Native subset v6 preserves them and reads v1-v5; older native tags cannot
claim bookmark extensions. Import remaps global block/region/source IDs; maths/cell/
part/series IDs remain scoped to their copied immutable content. Stale metadata stays
review-required after copy, and cannot become current through later monotonic edits.
Encrypted backup schema 11 preserves current/stale bookmarks and restores schemas
8/9/10; older backup tags reject the extension. Selection-only native exports include
only views whose bookmark endpoints are inside the selected block boundary. Workspace activation now discovers these saved views as described below; complete
authoring configuration and all-operation history remain open. These
are backend position contracts, not production caret rendering or gesture behavior.

## Last workspace and pane activation

`SaveWorkspace` stores the library-local pane order, integer relative weights,
horizontal/vertical arrangement, active slot (or None for the library route), selected
tool and references to existing saved views/PDF links. Stable slot IDs identify panes,
not entities. SQL identity guards reject slot/content ID collisions, including later
view/stroke inserts, so redacted placeholders cannot echo private target IDs. A saved view retains the current note/canvas/exercise identity, camera,
pen settings and revision-bound bookmark; ordinary entity reads resolve its notebook
ancestry. This is a backend activation contract, not production window or gesture code.
The default is an empty workspace at revision 0. Updates require the current workspace
revision and commit atomically; invalid layout, stale revision or access rejection
leaves the old layout intact. Eight panes and weights 1..1,000,000 are implementation
capacity/representation bounds, not approved performance SLOs. Selected-tool values
record intent only and never enable an unimplemented tool or start recognition.

`RestoreWorkspace` checks current app access first, then resolves every pane using
fresh view/source/block/exercise authority. Accessible view panes include their
selection restoration state and selected tool. Locked/unavailable panes expose only
opaque slot/weight placeholders, without target IDs, tool, titles or PDF locators;
independent accessible panes still restore. Protected targets are never automatically
unlocked. `Keep` retains only an existing slot's exact private target/tool, allowing
layout adjustment without revealing or rebinding it. Explicit new targets require
fresh access; Keep on a new/forged slot fails. Closing a locked pane edits navigation
metadata only and does not require unlocking/deleting its authored content.

PDF panes return authorised source/view metadata with
`requires_platform_validation=true`, even if stored availability is Available.
Restoration does not open a provider/parser, acquire a read lease, restore an OS grant,
read source bytes or claim runtime readiness. Regrant/rendering still goes through the
trusted PDF pipeline. Orientation/layout changes never transform source observations.
A workspace receipt acknowledges only workspace metadata, not active stroke capture.

Schema 12 adds relational workspace tables. Foreign-key retirement on permanent
view/source deletion replaces the target/tool with an unavailable placeholder and
advances workspace revision in the same purge transaction. Slot order, weight and
focus remain stable; Trash/unavailable contexts can recover their existing references
on ordinary restore. Encrypted schema-13 whole backups retain the schema-12 workspace metadata;
schemas 8-11 create the default empty workspace and cannot smuggle new workspace
tables under an old tag. Malformed references, order and table shapes fail closed.
Restored Available/Missing PDF sources remain Missing; an existing Revoked state
stays Revoked. No source becomes runtime-ready. Native v6 copies preserve selected saved
views but do not activate or overwrite a destination workspace; their omission report
states that activation is library-local. Complete pen presets/backgrounds, remaining
pane workflows and all-operation history/retirement remain open. No production H.I.
is authorised by this increment.

## Canvas layers and independent embed visibility

`CanvasLayers` is a revisioned source-canvas presentation state. The implicit base
layer contains every unassigned block; old canvases start with that base visible,
without rewriting existing blocks or source observations. Custom layers have stable
IDs, names, default visibility and disjoint block membership. Layer order precedes
per-block layout z/order. Removing a layer moves its members back to the base; it
never deletes working. SetCanvasLayers validates owner/kind, unique identities and
membership, current source/block access and optimistic revision. Its state changes
are atomic and undoable. Undo to the original implicit base retains a monotonically
advancing revision; redo cannot accidentally accept a stale revision-0 update.

`SetEmbedLayers` records either None (Use canvas defaults) or an explicit list of
Base/Custom layer references on that embed alone. An empty explicit list shows none.
Duplicate embeds retain independent overrides/camera/pin state. Changed canvas defaults
do not rewrite an explicit override. Deleted referenced layers are reported as
unresolved and stay invisible rather than falling back to all layers; undo can
restore the original identity. Fresh source/host/block protection still applies,
and layer visibility never grants access. Current embed reads retain their strict
whole-source permission requirement. Layer controls and edit history contain no
replacement copies of original observations.

`CanvasScene` projects visible items in layer/z/element-kind/ordinal order. Nested
embeds are currently base-layer items; inaccessible or broken references return
placeholders without their source/frame metadata. Source blocks with denied access
return protected placeholders without loading their content into the result. Scene
pages are bounded by 512 items and 8 MiB output, with explicit next_offset. An item
that alone exceeds the budget fails rather than truncating content. The metadata scan
is capped at 100,000 source items, including hidden items. These are implementation
capacity limits, not measured performance SLOs. Callers compare entity_revision,
layer_revision and embed_revision between pages and restart after a change instead
of combining incompatible snapshots. No renderer/provider or recogniser is started.

Schema 14 adds a source-owned layer-state table and durable undo capture. Native
subset v8 retains styles/bookmarks and reads v1-v7, remapping layer IDs, members,
embed references and even unresolved override identities without a live-source join.
Encrypted schema-14 backups retain state/history; schemas 8-13 default to the implicit
base and reject layer metadata/history smuggled under older tags. Purge retires the
source's state and detailed history. Default-visibility filtering applies to canvas
block derivative exports, with explicit omissions; raw native source remains intact.
Static embed/whole-scene rendering remains unsupported by those export paths.

Still open: layer-aware capture destination/recovery routing, custom assignment of
nested embed objects, local embed overlays/follow-object anchors, complete scene
rendering and other rich-object fidelity. This is backend layer/view behavior, not
production layer controls or a claim that the whole canvas programme is complete.

## Immutable chosen stroke appearance

New captures may supply `StrokeStyle { colour_rgba, width }`. Width uses the stroke's
named source-coordinate units, just like x/y; it is not silently interpreted as mm,
pixels or normalized pressure. The platform must explicitly convert its physical pen
configuration into that space. All channels are immutable and finite/bounded. A
contact snapshots style at begin. InputRouter/set_style, InkAccumulator/MotionCapture
and JournaledCaptureSession/setStyle change only subsequent contacts; active prefixes,
completion, interruption, exact retry receipts and original source do not rebind.
`openCaptureSession(..., style)` can establish the initial style. Style absence means
unknown legacy appearance, not an assertion that black/unit width was selected.
No current view settings or pressure sample is used to guess missing style.

Schema 13 versions this optional immutable source metadata without rewriting existing
payloads/hashes/receipts. Native subset v7 preserves it and reads v1-v6; older native
tags reject styled sources. Encrypted schema-13 backups preserve styles and restore
schemas 8-12 with their corresponding metadata limits. Android journal STEMAC02
stores exact RGBA bytes and width bits; STEMAC01 remains readable with style absent.
Recovery and explicit review preserve the begin-bound style; changed-style checkpoint
headers or receipt retries are rejected. These are chosen appearance settings, not
new sensor observations, pressure calibration or handwriting inference.

Original-ink SVG derivatives now honour recorded colour/alpha/base width, round paths
and single-observation dots. Bounds include stroke radius, avoiding clipped wide marks.
Legacy fallback appearance and unreconstructed pressure/tilt-dependent appearance are
reported in export warnings. Eraser observations are labelled as source paths rather
than silently replayed as destructive edits. Native source still retains every exact
observed point/channel/style independently of the derivative. Complete nib/pressure
rendering, physical-space mapping, presets/backgrounds and whole-scene export remain
open; these base-style derivatives are not a claim of full tablet appearance fidelity.

## Observed prefixes and stroke termination

Raw source strokes now distinguish `Completed`, `InProgress`, and
`Interrupted(Lifecycle|Recovery)`. Existing source payloads omit `end` and retain
completed semantics; default serialization still omits it, preserving old payload
hashes and capture receipt fingerprints. Active prefixes cannot enter SQLite,
normal capture queues or native documents. Interrupting seals only the observed
prefix and records its reason; no pen-up sample, coordinates or time is invented.

Rust `InputRouter` and Android `InkAccumulator` expose immutable `checkpoint`
snapshots with stable begin-allocated IDs/session order and explicit `interrupt`.
Snapshots remain uncommitted observations. Normal finish preserves identity and
records completion. Platform/palm cancellation still discards tentative contacts;
it is not inferred to be a lifecycle interruption. Routing/accumulation has bounded
active pointers (32) and a shared observed-point budget. Rejected batches preserve
prior observations. These are admission bounds, not tablet performance promises.

Android's generic DTO is now `CapturedStroke`; `CompletedStroke` remains a source
alias for existing callers. `CaptureRequest` rejects active prefixes and carries
explicit interrupted provenance through the existing JNI capture command. Ordinary
completion retains the previous wire encoding. `MotionCapture.interrupt` is the
backend lifecycle seam; the journaled session below connects checkpoint storage and lifecycle recovery.
Calling the accumulator's `checkpoint` alone is never a durable-save receipt.

SQLite schema 10 introduces the versioned source termination field without rewriting
legacy payloads. Native subset v5 carries interrupted provenance and reads v1-v4
completed sources; older native version tags cannot conceal the new semantics.
Encrypted logical backup schema 11 restores schemas 8/9/10 with the corresponding
legacy source semantics and preserves interrupted provenance/receipts. Unknown/active
termination or downgraded containers fail closed. This remains the implemented
native subset; complete PDF-linked portability and scalable whole backup are open.

### Durable journaled capture sessions

`openCaptureSession(entity, coordinateSpace)` returns a backend-bound input session.
Its `accept(MotionEvent)` returns immutable observations plus per-stroke futures.
`Receipt.checkpointed` acknowledges durable recovery storage (or a verified earlier
commit); `committed` additionally
acknowledges the SQLite capture receipt. Neither a completed future nor a raw snapshot
alone means the document was saved. A rejected submission retains its observations
in the returned value for explicit `retry`. `interrupt` seals current observed
prefixes with Lifecycle provenance; full backend close also freezes and attempts all
live sessions after draining already admitted work. A failed closing checkpoint is
reported while unrelated sessions still get their own sealing attempt. Caller-owned
interrupted observations remain available when the filesystem cannot accept them.

The journal is private in `noBackupFilesDir`, bound to the native library UUID,
entity, coordinate space, stable stroke/session identities and monotonic revision.
Its versioned binary records preserve IEEE double bits and unavailable channels,
use SHA-256 integrity checks and exact prefix validation, fsync file/rename/directory
boundaries, and sync new directory ancestry before acknowledgement. Default bounds
are 32 MiB/256 records, 6 MiB per record and one atomic replacement temporary. These
are engineering limits, not measured device/RSS or save-latency guarantees. No old
record is evicted to admit newer input. Valid interrupted temporaries are reconciled
by revision/prefix; torn unacknowledged temporaries remain separate `.incomplete`
evidence and raise attention. Invalid authoritative or foreign-library records fail
closed without deletion or implicit rebinding. These files are not portable backups.

Native capture receipts and session sequence admission reject stale active frames
that would recreate committed or discarded contacts. Platform cancellations have
explicit durable discard handling and never publish tentative ink. Replay preserves
each session's order; active/unreviewed/failed sessions do not obstruct unrelated
completed input. Legacy outbox replay remains supported; its integer replay count is not a per-ticket
commit acknowledgement. Use the scoped `CaptureReceipt` query or the journaled
session's explicit `committed` receipt for that decision. Cancelling an observation
future does not cancel an admitted writer action, including legacy captures, generic
requests and recovery decisions. API rejection still leaves data with the caller.

Startup reconciles the journal and replays already sealed/confirmed records. Surviving
active prefixes become `Interrupted(Recovery)` evidence requiring review, not invented
completions or silently published palm contacts. `interruptedCaptureTickets` filters
by current native context access. `reviewInterruptedCapture` requires current access;
accept/discard requires explicit confirmation, the same backend owner, a fresh
checksum and another access check. Native commit rechecks permissions too. Unlocking
retries already confirmed work without treating successful authentication as successful
persistence. Opaque JNI recovery identity is available for binding while locked and
grants no document access; `CaptureReceipt` is itself scoped. `captureRecoveryState`
exposes counts/attention without raw strokes. Detailed all-operation retirement,
Android priority/byte admission/status integration and physical-device interruptions
remain open; pending/rejected data is never given a zero-loss guarantee.

## Android backend library

Provision JDK 17, Gradle 8.9, Android SDK platform/build tools 35, NDK 27.0.12077973 and
Rust target `aarch64-linux-android`. Keep these build tools outside Git. Then run:

```sh
rustup target add aarch64-linux-android
python3 tools/validate_android.py --sdk "$ANDROID_HOME" --jdk "$JAVA_HOME" --gradle /absolute/path/gradle
```

The first provisioning run can add `--online` to fetch development dependencies.
The validation runner builds ARM64, checks ELF, exercises the actual JNI library with
`-Xcheck:jni`, builds the Android AAR, runs unit/Robolectric tests, runs lint and checks
the merged manifest. It stores exact commands/results and source hashes. The AAR
is not an installable notebook and contains no production H.I. No device/emulator
execution or hardware biometrics success is implied by these tests.

`NotebookBackend` serialises JNI requests off the Android main thread and stores the
library below `noBackupFilesDir`. Preserve unacknowledged capture data when admission
fails. Authentication accepts only a trusted platform-verifier callback, never a JSON
flag. Relock on the appropriate app lifecycle boundaries. `MotionCapture` assembles
observed pointer lifetimes. Supply completed strokes to `NotebookBackend.capture` with
a stable ticket chosen before submission. A full executor rejects admission; retain the
observations in the caller until admitted. The off-thread durable outbox keeps exact requests
until `ok=true`, stops replay on the first failure and survives restart. `retryCaptures`
resumes after opening/unlocking or a recoverable storage error; `pendingCaptures` reports
remaining records. The returned replay count is not a per-ticket commit receipt.
A low-space check releases the reserve before writing. Call `allocateReserve` when space
is available. Active-stroke interruption, device behaviour and capture-priority scheduling
still require implementation/acceptance; no zero-loss or performance guarantee is implied.

`PdfProvider.openReadOnly(uri, timeoutMillis)` now returns an owned `PendingPdf`.
Opening runs on bounded background workers using a `CancellationSignal`. Call `await`
off the main thread to take the read-only descriptor, then close that descriptor;
close/cancel the pending operation if it is abandoned. Provider close also retires
unclaimed descriptors. Timeouts and cancellation retain admission while an
uncooperative provider is still running, and late descriptors close without delivery.
A ready but unclaimed descriptor also consumes capacity. Cancellation listeners have
a separate bounded executor so a blocking listener cannot stall deadline delivery.
Grant retention remains explicit. Revocation cancels current openings before releasing
the platform grant; the core/client must independently enforce persisted source policy.
No source bytes are copied by this adapter and its errors omit private provider details.

`NotebookBackend.renderPdf(source, page, width, height)` now connects the native
source policy, provider and isolated parser. It returns an owned `PendingFrame`;
call `await` off the main thread, then use `Frame.draw` for the viewport. Closing,
source/lock/containment changes, failed authentication and native access expiry
retire earlier frames. `Frame` never exposes its Bitmap and rejects drawing once
retired. Ordinary pen/text edits keep the PDF viewport; no canvas or ink is changed.
`closeAsync` acknowledges native writer drain. An uncooperative platform callback
may still return late, but its private results cannot become a new preview.

Each read streams into an app-private, disposable cache below `noBackupFilesDir`
while hashing the exact source revision. This freezes the parser input without
turning a link into a managed library copy. Cache paths are unlinked before parsing
and crash leftovers are removed on startup; these bytes are never included in native
backup/export. Streaming uses bounded buffers, a 16 GiB operation bound and an 8 MiB
free-space guard for disposable copying; these are engineering admission limits,
not approved device performance or zero-loss SLOs. Provider access is reopened
read-only and native revision/access are revalidated before frame publication.

The native JSON v1 `Pdf` command group exposes Source, Link, SetView, MarkMissing,
Revoke, Anchor, CreateAnchor, ReconnectAnchor, ReviewRevision, ReplaceRevision,
PrepareRead, ValidateRead, ReleaseRead and ReleaseSelection. Use `{"op":"Pdf","command":{"op":...}}`
inside the usual request. Prepared read leases and revision-review tokens refer to
private bounded host state (32 each, at most five minutes); reviews are single-use,
and read validity is additionally bounded by remaining authentication lifetime.
Ordinary requests cannot supply a review plan or a `new_platform_grant` flag.
Relock/access changes clear prepared capabilities. Changing source revisions keeps
anchor lineage and requires an explicit prepared review rather than guessing a page.

Trusted source selection uses `reviewPdfSelection(Target.Link(context) or
Target.Relink(source), pickedUri, pickerFlags)` and a separate explicit
`acceptPdfSelection(review)`. The returned objects are not serialisable grants.
Provider opening and signature/SHA-256 observation run on one bounded optional
worker with timeouts and cooperative cancellation. Preparation does not write a
source or retain a persistent provider grant. Rejection/expiry/relock retires the
review. Acceptance reopens and rehashes the chosen source; changed bytes, permission
loss, stale native generation or current locks reject the old proposal. Persistable
read access is requested only after explicit acceptance and only when the picker
provided its flag; write access is never requested. System grant retention and SQLite
commit are separate boundaries: a failed database save may leave a deliberately
retained platform read grant, never a falsely acknowledged source save.

Separate trusted JNI prepare/finish methods verify a fresh one-use observation
callback, bound to destination, locator and hash. They do not perform provider I/O
on the native writer. Private native selection reviews are bounded to 32/five minutes,
are single-use and check current library/generation/access. Both same-revision and
changed-revision selection can recover a revoked or restored source while retaining
its ID. A changed edition preserves prior revisions and marks affected anchors
historical or needing review rather than relocating annotations. Independent retained
evidence and authored working survive. New links have a proposed ID before commit,
so an ambiguous process interruption can be reconciled by that identity.

Before native commit acceptance, cancellation prevents the observation proof from
authorising a late write. Once the writer accepts the fresh proof for an explicitly
confirmed commit, cancellation no longer invents a failure or rolls back a successful
save: the caller waits for the actual receipt/error. That receipt contains the known
source ID, not a cached private locator/title. Provider failure is published even if
provider retirement or the selection deadline thread is blocked; admission remains
reserved until resources actually retire. The parser client also fails closed after
a disposal/unbind failure rather than admitting unbounded additional processes.

The parser binds to a fresh isolated service instance. Only a duplicated read-only
FD, page, viewport dimensions, request token and pixel format cross this boundary;
no notebook path, source locator or authentication token does. A separate control
looper handles termination while parsing runs elsewhere. Timeout/cancellation
closes descriptors, sends stop and unbinds; an isolated service also terminates on
destruction. Admission is retained while an outstanding bind/send/read/disposal
still runs, even after the caller has received a timeout. Late replies close their
descriptors. Input ownership remains with the caller; the client owns its duplicate.

Private pixel IPC format `stem-argb-1` is `STEMPIX1`, big-endian u32 width/height,
then exactly width*height big-endian ARGB u32 values and EOF. The host checks the
requested dimensions before allocation (maximum side 4096, total 8,000,000 pixels),
reads bounded rows and rejects truncation/trailing bytes. It does not run a PNG or
other image decoder over worker output. The worker closes both local pipe ends on
failed reply/write. This is PDF viewing, never a raster handwriting-recognition path.

Automated fixtures use actual desktop JNI with simulated Android/provider/Messenger
boundaries. Robolectric's pipe/dup shadow uses regular files/copies, so these tests
are not live kernel-pipe or cross-process/device evidence. The module explicitly
pins Build Tools 35.0.0. Complete external permission/lifecycle/navigation
cases and actual service/device execution remain open. Picker/regrant paths are
backend contracts and synthetic platform/JNI fixtures, not actual device picker or
grant-persistence acceptance. Already drawn external H.I. pixels must be hidden/redrawn
when a frame is retired; this library does not implement production H.I.

### Retained PDF assets

Explicit native `Pdf` commands now support managed copies and independently
authorised full-PDF snapshots. `BeginRetention` takes `source`, `class`
(`ManagedCopy` or `HistoricalSnapshot`), nullable `authority`, `export_allowed`,
`consented` and exact `length`. Historical retention and any export permission
require a separate nonblank authority; this text records a deliberate retention
decision and never manufactures copyright or platform permission. No provider
access or automatic source copying is implied by a source link.

The returned private `transfer` accepts sequential `AppendRetention` chunks
(`offset`, numeric-byte-array `bytes`, maximum 65,536 bytes). `FinishRetention`
requires explicit `confirmed: true`, exact length, PDF signature and SHA-256 match
to the still-current source. Rejection, bad chunks, stale source policy or relock
consume/retire the transfer. `ReleaseRetention` is idempotent. Staging has one
64 MiB maximum in-memory reservation per native host, expires after five minutes
on the next host operation, and never writes a partial asset to the library or
backup. These are engineering bounds, not measured tablet limits. A failed save
has no success receipt; after an ambiguous outcome reauthenticate and inspect
`Assets` rather than blindly creating another copy. `NotebookBackend.retainPdf(source, decision, confirmed, timeout)` is the explicit
Android step after accepting a picker link/regrant. `ManagedCopy` defaults to no
independent authority/export; `HistoricalSnapshot` requires a separately recorded
nonblank authority. Both reject export permission without independent authority.
Unconfirmed calls do no provider/native work. Link acceptance and retention have
separate receipts: a failed copy preserves the accepted link and requires an
explicit retry; there is no silent fallback or automatic local duplication.

The adapter shares the selection worker/admission slot. It preflights a native read
lease, freezes/hash-checks provider bytes into a private `noBackupFilesDir` cache,
streams native chunks, unlinks the frozen cache, reopens read-only permission and
validates native policy before submitting the final commit. Large known descriptor
sizes are rejected before copying; unknown-length streams are counted as read.
The copy limit is 64 MiB with bounded buffers and an 8 MiB temporary free-space
guard. Read validity is conservatively anchored before the native request so
response delay cannot extend authentication. Cancellation/deadlines retire pending
work and hold admission until late resources finish. Once final submission begins,
the adapter awaits the actual receipt/error rather than inventing a cancellation
or timeout for a potentially committed save. No provider call occurs on the native
writer and retention obtains no additional platform persistable permission. Failed
disposable-cache removal closes acquisition instead of admitting more copies;
authored notebook operations remain independent.

`Assets {source}` lists checked metadata (maximum 256 rows per source; excess
fails closed). `PrepareAssetRead {asset}` returns a private read lease and metadata.
`ReadAssetChunk {lease,offset,length}` reads at most 65,536 bytes, checking current
context access and the exact asset metadata each time. `ValidateAssetRead` takes
`observed_digest` and rechecks policy/hash; `ReleaseRead` also retires asset leases.
Live and retained read leases share the same 32-capability bound and five-minute/
authentication lifetime. Disposable caches are never eligible for retained-PDF
reading. Managed copies without independent authority remain usable while a source
is missing, but are purged on source revocation or replacement. Independently
retained assets preserve captured source revision and permission provenance.
`RevokeRetention {asset}` withdraws that separate authority and deletes its bytes.

`NotebookBackend.renderRetainedPdf` reads checked chunks into the disposable private
cache, verifies their complete digest, unlinks the frozen file before isolated
parsing, and validates the lease again before publishing. No external provider is
opened. Frames expose the retained asset ID, captured `sourceRevision` and
`historical` flag (independent retention is displayed as historical evidence).
Retirement, expiry, relock and retention revocation use the same private bitmap
ownership as live reads. Already drawn H.I. pixels still require eventual H.I.
hide/redraw handling; no production H.I. is implemented here.

### Library ownership and full shutdown

Android reserves the canonical root in process before opening its OS file lease
in `noBackupFilesDir` or constructing any
workspace or retiring crash files. Duplicate/canonical-root aliases fail before
factory or cleanup code can run; a symlinked notebook directory is rejected.
Rejecting in process before opening a duplicate FD also prevents that rejected
channel's close from releasing the process's POSIX lease. An independent JVM probe
checks actual OS ownership after rejected duplicates and after full retirement. The
persistent empty owner file is not unlinked, avoiding a split-inode lock race.
Factories finish before any JNI handle opens, and failed initialization closes
already-created resources/provider and retires ownership. Asynchronous native-open
failure also closes the backend. Capture recovery records remain authoritative and
are not discarded by these paths.

`closeAsync` remains the native writer-drain acknowledgement.
`closeFullyAsync` additionally waits for provider opening/cancellation, PDF parsing/
disposal/control threads, cache/selection workers and owned descriptors before
releasing app ownership. An uncooperative platform call leaves full shutdown pending
and a second backend is rejected; late bytes cannot publish or let another instance
retire an active cache. Reopen after the full acknowledgement. The closer runs away
from main/writer/control threads. Independent notebook authoring and original ink
retain their existing commit/recovery semantics. No device termination-latency
claim follows from these synthetic lifecycle fixtures.

## Rendering and processing

Use `prepare_derivative(selection, false, access)` followed by `finish_pdf_export` for
converted-only PDF. The plan is tied to library identity/generation and protected
scopes; finishing reauthenticates and rechecks after rendering. Pure `render_pdf`
accepts already-authorised content and a local font, not a database or arbitrary SVG.
Cropped images, static embedded-canvas fidelity and full scene rendering remain open.

`partition_regions`, `split_region`, `merge_regions` and `region_snapshot` keep source
observations in their authoritative block. Engine results remain pending proposals
until explicit acceptance. Mock tests do not establish model accuracy. Resumable
resource transfers and leases are available in `provisioning.rs`; integration into the
complete Android model manager is unfinished.

## Discovering saved writing views

`Views {entity, exercise, offset, limit}` returns existing authorised view IDs in
stable ID order. `exercise: null` selects master views only; an exercise ID selects
that exercise's views and must belong to the entity. Restore the chosen ID with
`RestoreView` for fresh access checks and explicit bookmark current/review state.
No view or default position is created by discovery. Closed views remain discoverable
after restart independently of last-workspace panes.

Pagination counts visible views only and omits identifiers whose anchor/bookmark or
exercise is currently inaccessible. It returns at most 512 IDs, scans at most 100,000
candidates per request, and rejects offsets above 100,000. These are implementation
capacities, not performance guarantees. Start again when views or access change;
separate requests do not form a frozen snapshot. Stored key/owner/payload disagreement
fails as integrity corruption rather than returning a different view.

## Recovery evidence for retired targets

`CaptureRecoveryAccess {entity}` returns only `state: Active|Trashed|Purged`; it
does not return document metadata or grant content/capture access. Normal read and
capture commands still reject trashed/purged targets. Recovery authority checks app
and all current/inherited entity locks, including purged ancestor tombstones. Any
retired target additionally requires a fresh target-specific Entity authentication
grant, even when it used to be public. Missing/misbound identities fail closed.

Android `interruptedCaptureTickets`/`reviewInterruptedCapture` now cover all sealed
pending journal records, including completed/lifecycle captures blocked by saving,
and expose the reviewed `targetState`. Active live contacts are excluded. Review
retains exact observed ending, style, coordinate space and samples. Explicit discard
checks fresh recovery authority and the reviewed record checksum before deleting only
that recovery copy. It neither undoes an ambiguous document commit nor bans a later
explicit resubmission by the original caller. It does not discard other session
records. Confirmation is required. Live source payloads are not returned by the
recovery-access command.

Accept still requires normal current document authority before journal approval/replay.
A retired target cannot be silently restored, retargeted or replayed. After an explicit
Trash restore, the unchanged draft may be reviewed/accepted normally. Purged drafts
remain reviewable/discardable under the scoped proof. Recovery is private bounded
no-backup storage, not an export or encrypted working-database claim. Legacy capture
outbox and corrupt/incomplete records still need their own full recovery workflows.

Core subtree purge now preflights every descendant's current access/content protection
before changing any ancestor to a tombstone; all deletion remains one transaction.
Host-facing deletion uses the opaque review workflow below; caller-forged serialized
DeletionReview objects are never accepted.

## Trash and permanent deletion

1. `PrepareDeletion {entity, permanent:false}` reviews ordinary deletion to Trash.
   It returns a host-local opaque `review` token, the action kind, dependency-presence
   flag and IDs of currently visible dependants. Protected referencing IDs/titles are
   omitted; a boolean can still report dependencies. Full subtree content authority
   is required. Preparation changes no document data.
2. `ConfirmDeletion {review, confirmed:true}` consumes that review, checks the exact
   reviewed library generation and current subtree authority, and commits one atomic
   operation. Explicit confirmation is required even without dependencies. False
   confirmation leaves the review available; any true confirmation attempt consumes
   it, including stale/denied/faulted attempts. Prepare again after an error. The
   response with ok=true is the commit acknowledgement; an ambiguous transport reply
   requires inspecting current Trash state rather than assuming success or failure.
3. `TrashEntries {offset,limit}` discovers explicitly trashed, non-purged entities
   with current entity/app/ancestor access. Visible-only pagination uses stable ID
   order, at most 512 entries and 100,000 scanned candidates/offset. An inherited
   Trash state does not manufacture a separate entry. Pages are not frozen snapshots.
4. `RestoreEntity {entity,revision}` restores the listed entity only at that exact
   revision and advances it. Restore ancestors first; independently trashed children
   remain trashed. It does not rewrite content, bypass locks, restore purged content
   or act as edit Undo/history/whole-library backup recovery.
5. For deliberate permanent deletion, prepare with `permanent:true` for an explicitly
   trashed target, review dependencies again, then confirm that token. Purge atomically
   retires subtree content/assets/detailed undo while keeping honest tombstone IDs.
   It is not an overwrite of a separate healthy recovered library.

Use `ReleaseDeletion {review}` to cancel a review. Tokens are non-persistent, expire
after 300 seconds and are limited to 32 outstanding per host. They are cleared by
authentication replacement, relock/expiry and protection/containment/undo changes;
reopen or a different host cannot use them. Other committed changes invalidate their
generation. These limits are implementation capacities, not performance promises.
Review bounds are 100,000 subtree identities and 4,096 visible dependant IDs; oversized
reviews fail, never silently truncate consent. The recursive identity query can still
allocate before that size check; these bounds do not claim constant RSS or latency.

Trash/purge handle independently trashed and already-purged descendants coherently;
purge preflights remaining live payload authority before tombstoning ancestors. Source
ink stays intact through Trash/restore and is retired only by authorised permanent
deletion. Ordinary source/preview access is revoked while trashed; native and Android
PDF leases/previews retire conservatively on deletion confirmation and restore.

Private pending capture drafts are separate recovery evidence. Deletion never silently
discards or retargets them; the scoped review/discard path above remains available.
A combined deletion preview showing pending-draft disposition is still open, as are
complete all-operation history, recovery and device privacy acceptance.

## Fixed canvas-layer capture destinations

`AppendTo`/`CaptureTo` accept `target: {"CanvasLayer":{"canvas":ID,"layer":"Base"}}`
or a Custom layer object in place of Base (`{"Custom":ID}`). Existing Entity and
Exercise targets keep their encoded receipt identity. New blocks and custom-layer
membership commit in the same transaction/undo operation. Membership advances the
layer revision, without changing stroke coordinates, ending, style or ownership.
Current canvas and whole-source protection apply; layer visibility grants no access.
A hidden explicit destination stays that destination. Missing custom IDs require
review and never fall back to base or another layer.

Exact committed ticket retries validate current owner/block access before returning
the existing receipt, even if the original layer was subsequently removed. They do
not recreate membership or append again. Changing a target with the same ticket
conflicts. A new ticket for a missing custom layer rejects atomically.

Android `openCaptureSession(entity, coordinateSpace, style, layer)` accepts optional
CaptureLayer.Base/Custom. The default null retains legacy Entity routing. A session's
destination is immutable: interrupt it and open a new session when selecting a
different layer, preserving all old submissions/retries at their original destination.
Journal format STEMAC03 stores the destination independently of the immutable stroke
header. It reads STEMAC01/02 as legacy Entity destinations without inventing a layer.
Startup replay, normal checkpoint, retry and graceful close all use that fixed value.
Legacy valid request bytes remain unchanged; malformed UTF-16 coordinate spaces now
reject before UTF-8 replacement can corrupt their identity.

Recovery reviews expose the recorded layer and its current availability under fresh
source authority (true/false, or null when current layer metadata cannot be read).
Unknown layer metadata does not prevent reviewing independently authored draft ink
under its own context locks, and never authorises acceptance. Missing destinations remain reviewable and explicitly discardable,
and acceptance checks current availability before approval. Restoring the same layer
identity can make the original destination available again; there is no automatic
retargeting or invented source sample. Deliberate retargeting to a different layer
requires a future reviewed workflow and is not exposed here. Existing capacity,
resource/lifecycle and production-H.I. gates still apply.

## Capture receipt privacy

Receipt retry checks authorise the stored block's current metadata, actual owning
context and current block/exercise protection before fetching/comparing the stored
fingerprint. A syntactically valid changed retry cannot confirm a guess about
protected ink by producing a different conflict error before authentication. Naming
a different public destination does not replace the stored block's authority. The
current payload identity and expected owner are checked before returning a receipt.
`CaptureReceipt` likewise gates metadata before deserialising block content. Common
block reads now apply that metadata-first gate and verify the payload ID and SQL owner
binding; a working-row payload spliced into another owner fails closed.

Authorised identical retries still return the existing block; authorised changed
payload/target retries conflict. Existing fingerprints and source bytes stay intact.
Request validation errors concern supplied input and still run before persistence.
These rules address structured receipt replies, not a constant-time implementation
or complete privacy acceptance for every backend path.

## Combined deletion and pending captures

Native PrepareDeletion now returns capture_targets: currently authorised, non-purged
Note/Canvas IDs in the reviewed subtree. Protected/purged descendants do not acquire
new preview rights. ValidateDeletionReview rechecks the held token's generation and
current subtree content authority without consuming it; confirmation still rechecks
and consumes its native token. The 32-token/300-second review limits remain.

Android prepareDeletionWithCaptures(entity, permanent) combines that native review
with exact checksum-bound recovery records from both the active journal and recognised
legacy Capture outbox records. It returns dependency metadata and draft tickets/owners/
sealed flags, never raw observations. Current recovery access is checked for matched
records, including the separate target-specific proof for already-retired contexts.
Unattributable legacy records and incomplete files are counted without owner/content
disclosure and always retained. Other contexts' drafts are excluded.

confirmDeletionWithCaptures(review, disposition, confirmed=true) is one admitted writer
operation. It validates native authority first, then exact current record hashes, then
commits native deletion. Any draft-prefix/membership change since preview rejects and
releases the review without deleting/discarding. Access/relock, generation and expiry
checks still apply. Use releaseDeletionWithCaptures to cancel before confirmation.
False confirmation rejects before admission; true attempts consume the review.

KeepDrafts preserves all recovery copies. DiscardReviewedSealed removes only the exact
reviewed sealed copies after successful native acknowledgement. Active, unknown,
unrelated and later-arriving drafts stay retained; they are not retargeted or completed
synthetically. The successful native acknowledgement authorises disposal of that
reviewed subset as part of the same operation, even when the context has just become
Trash/purged. This grants no subsequent raw-content access to a retired context.

DeletionCaptureReceipt acknowledges the document action separately from draftsRemoved,
keptActive, keptSealed and cleanupUnconfirmed. If draft cleanup fails after native commit,
the document action remains committed; cleanupUnconfirmed counts removals lacking an
acknowledgement, which may include an uncertain filesystem result. Attention is set.
The app must not claim those copies were removed or roll back the document implicitly.
Use scoped recovery review/discard to resolve retained evidence. Failed/ambiguous native
acknowledgement removes no recovery copy and requires checking current document state.
Cancellation of a returned future cancels observation, not the admitted operation.

SQLite and recovery files are separate durability boundaries; this is not an atomic
cross-store deletion or a journal-export/undo API. Existing raw JSON deletion commands
remain document-only and retain private recovery evidence by default. Production H.I.
should use the combined typed review. Legacy records not recognised as Entity Capture
are retained; full legacy/incomplete content review/recovery remains open. Existing
record/byte capacities bound this workflow, with no constant-RSS or latency claim.

## Per-view presets, calibration and guide backgrounds

ViewSettings optionally contains authoring: {presets, selected_preset, background}.
Presets have view-local IDs, name, width_mm and RGBA. At most 32 are accepted; names
are nonblank and at most 256 UTF-8 bytes, widths finite and in (0,100] mm, and selected
IDs must exist. These capacities are not performance SLOs. SaveView validates selected
appearance against current pen fields. ConfigureView {view,expected,authoring} applies
the selected preset atomically; ApplyViewPreset {view,expected,preset} selects one.
Clearing the marker keeps the current custom appearance. Missing IDs, stale revisions
and invalid values reject without silent repair/clamping.

Preference changes preserve camera, anchor, finger setting and exact bookmark bytes,
including a stale bookmark's NeedsReview status. They do not consume an edit-Undo
step or edit existing strokes. Fresh view/entity/exercise/anchor/bookmark authority
still applies. Reopen/selected native copy/encrypted backup retain configuration.
View-local preset IDs remain local when a view is copied; global view/entity IDs remap.

ViewPen {view,coordinate_space,units_per_mm} returns an authorised configuration
snapshot and StrokeStyle with width = pen_width_mm * the caller's explicit scale.
The host supplies calibration for its named source coordinates; no DPI, accuracy or
physical device mapping is guessed or measured here. Invalid/nonfinite/overflow or
out-of-range widths reject. Apply a new snapshot to future contacts using the existing
setStyle behavior; active headers and raw measured points stay unchanged. Pressure
response, eraser configuration and full device calibration acceptance remain open.

Backgrounds specify Blank/Ruled/Dotted/Graph, positive spacing_mm <=1000, positive
line_width_mm <=spacing, finite origin_mm, guide/paper RGBA and include_in_export.
ViewGuides {view,bounds,coordinate_space,units_per_mm,for_export} returns a standalone
SVG component in those source bounds, with explicit revision/configured/omission
metadata. SVG patterns bound output size; logical guide density is capped at one
million lines/dots. Input coordinate-space labels are XML escaped. Missing legacy
configuration yields no invented background; for_export with include_in_export=false
yields no SVG. Blank may still include a chosen paper fill. Guide output never reads
or embeds source strokes. Full scene/PDF export does not automatically composite
this component; that renderer integration remains open.

Schema 15 marks the optional authoring extension without rewriting existing payloads.
Native subset v9 reads v1-v8, rejects authoring under an older tag, and preserves
view-local presets/backgrounds. Encrypted schema-15 backups restore schemas 8-14;
older schema tags cannot hide authoring metadata. Absent authoring serialises exactly
as before, preserving legacy view bytes. Production H.I. remains gated.


### Explicit Android finger drawing

`openCaptureSession(entity, coordinateSpace, style, layer, fingerInk=false)` keeps
finger contact out of source ink by default. A host can supply the current authorised
`ViewPen.finger_ink` setting explicitly; reading settings does not grant capture access.
`session.setFingerInk(enabled)` changes eligibility of future contacts only. A finger
contact already admitted keeps its original header until completion/cancellation.

Opted-in contacts encode the existing native `ExplicitFinger` tool, never `Pen`.
Stylus/eraser arrival cancels tentative active finger strokes; fingers beginning during
an active stylus/eraser are ignored. Previously committed finger strokes are unchanged.
Android cancellation still discards only the corresponding tentative observations;
lifecycle interruption preserves exact prefixes. This backend policy uses reported
Android tool types and is not evidence of physical palm rejection or H.I. navigation.

Private active-journal format **STEMAC04** retains v3 layout and replaces its eraser
boolean with a strict tool byte: 0 Pen, 1 Eraser, 2 ExplicitFinger. Readers accept
STEMAC01–03 with their original 0/1 meaning and absent older style/layer metadata.
Rewriting a recovered legacy record uses v4 without inventing finger classification.
Tool identity is immutable across checkpoint revisions and survives review/replay.
SQLite/native document formats already support ExplicitFinger and are unchanged.


### Reviewed pending-draft layer retargeting

For sealed active-journal evidence, `reviewCaptureRetarget(sourceReview, destination)`
prepares an explicit same-canvas destination and current canvas-layer revision.
`confirmCaptureRetarget(review, confirmed=true)` freshly checks source-context access,
active target state, exact journal checksum, absence of an already committed SQLite
receipt, absence of a conflicting legacy envelope ticket, destination existence and
unchanged layer revision before altering the private pending route. Current full-canvas
permissions apply even for Base. Retired targets and Note destinations are rejected.
The original session cannot implicitly select a replacement for a missing layer.

Only route/recovery revision metadata changes; stroke ID, capture session, begin order,
coordinates, style, tool and observed ending remain identical. The journal persists the
selected route before approving recovery and attempting normal per-session FIFO replay.
An earlier blocked stroke may still defer publication. The typed receipt separately
reports private checkpointing and actual document commitment; SQLite and the private
journal remain separate durability boundaries. Errors after durable publication require
inspecting recovery/current document state, not assuming rollback or a document save.

Changed/missing layers, changed recovery evidence and fresh protection reject without
implicit fallback. A committed or ambiguously acknowledged original ticket can only
replay its exact original target/source; it cannot be retargeted. A caller retrying an
old session frame probes an existing SQLite receipt before recreating a missing journal
file, preventing a retired frame from leaving a stale pending destination behind.
Cross-entity moves and full legacy/incomplete-record retargeting remain open.


### Guarded model-resource reader handles

`ResourceLeases.acquire(path, expected_sha256)` continues to verify explicitly supplied
local resource bytes before admission. `ResourceLease.file()` now returns another
owned `ResourceLease` implementing Read/Seek, rather than an untracked owned File.
Each successful clone holds the shared pool reservation through transfer to another
worker, even if the original lease is dropped. Like File::try_clone, cloned descriptors
share a seek position and readers must coordinate. The descriptor closes before its
reservation is released; pool removal conflicts until the last guarded reader retires.

The Rust reader return type changes deliberately; there are no production callers of
the old raw-file accessor in this backend. No raw owned descriptor extraction is added.
Workers must use the same shared lease pool. This component fix does not itself wire
recogniser adapters, mapped native resources, scheduling or isolated-process shutdown
into the host. Those lifecycle integrations remain open. No resource installation,
model download, executable plugin loading or handwriting egress occurs here.


### Discarding a caller-held recovery frame

Explicit sealed-draft discard retires the exact caller-held session ticket after fresh
scope/checksum/ending validation and before unlinking its journal file. The existing
per-session begin-order check then rejects old completed/active frames when no matching
journal or SQLite receipt exists. No separate tombstone collection or new persistent
format is added, and other pending tickets retain their normal retry path.

The backend keeps weak keys for caller-owned sessions so an interrupted session still
held by its caller receives the same retirement. This does not extend session or raw
observation lifetimes, change the active-session limit or add strong recovery ownership.
Both standalone discard and acknowledged combined deletion/disposition use the same
validated pre-removal hook. Failed/ambiguous native deletion never invokes it. A failure
after file unlink still rejects old typed frames, while reporting disposal failure.
Uncertain filesystem durability after a failed directory sync remains uncertain; no
power-loss guarantee or global prohibition on deliberate future reimport is inferred.


### Stored recognition proposal authority

The Store proposal reader checks the current app lock, then authorises the source
block named by the SQL relation before reading/parsing private transcription JSON.
Current block/exercise/ancestor protection therefore precedes malformed-payload errors.
Payload proposal/source IDs must match the requested SQL key and source relation.
Mismatch is an integrity error even for a caller authorised to inspect the real source;
correction/rejection/acceptance cannot treat a payload ID as a different row key.

This is a stored-row binding/corruption defence, not encryption or protection against
an attacker with arbitrary database read/write access. Existing source fingerprints,
proposal revision checks and preservation of original ink remain unchanged. The native bounded recognition/proposal path is described below; an eligible
recogniser and Android provider/job integration remain open.


### Owned recognition and conversion work

Manager.prepare_recognition and prepare_conversion return move-only work with owned
input, immutable descriptor provenance and a pinned verified adapter. The manager is
not borrowed while Work.run executes, so the owner can change defaults/remove/swap
versions without holding a manager lock around inference. Preparing recognition binds
the selected current engine; changing the default does not silently choose a different
engine for that work. Conversion format callbacks execute inside the worker panic
boundary, not during preparation. Existing synchronous convenience methods delegate
to the same worker path.

Entry removal, successful replacement, explicit readiness revalidation and manager
shutdown invalidate that entry's old epoch. Linked JobControl checks retain caller
cancellation/deadline and any parent epochs; cooperative adapters observe invalidation,
and a late successful return is still rejected. A failed staged replacement probe or
SQLite commit leaves the previous verified entry/work valid. Explicit failed
revalidation instead clears its prior Ready state and cancels old work.

The adapter/resource lifetime remains pinned until the work actually retires. Logical
cancellation does not claim hard termination of a non-cooperative native runtime or
release its resource reservations early. The host must reserve worker/memory capacity
before preparing work and freshly authorise/revision-check source before publication;
this API is not a queue, permission grant or new JSON request. Store.propose still
creates a pending review, and acceptance remains explicit with immutable original ink.

Native host transport and current-scope cancellation now use the bounded path below.
JNI/Android recogniser-provider integration, real eligible recognisers and isolated-worker
termination/scheduling remain open. No model, SDK,
certificate, weights, executable plugin or network permission is introduced here.


### Bounded native recognition and review

Trusted Rust callers request Host.recognition_source(block, optional region), prepare
that move-only source with a verified Manager and run it on an optional worker. The
writer snapshots authorised ordered digital ink; it never invokes recognition callbacks.
Host.publish_recognition accepts only a native completion bound to that host and source
epoch. JSON cannot submit recognised output, register an adapter or claim runtime readiness.

Each host reserves at most min(8, query count capacity) source slots and 32 MiB of
canonical encoded source bytes, including prepared, running and retiring inputs.
Individual snapshots/results/publications are bounded to 8 MiB including admission
charge. These are engineering admission limits, not model RSS or latency guarantees.
Status exposes only reserved/running counts and closure. No content or source identifiers
are included. The optional retirement worker starts lazily; its startup failure does
not disable library opening or ink authoring.

Authentication attempts, relock/expiry and successful protection/move/history/deletion
changes invalidate old source epochs. Exact access expiry remains attached to the job;
a longer inference budget cannot extend it. Caller cancellation/deadline and runtime
retirement also apply at publication. Fresh SQL source access and revision/fingerprint
checks precede initial publication. Inference may return late but cannot publish stale
success. Cancellation retires unrun source/runtime objects off the writer. Running work
keeps its quota and library ownership until actual callback/resource retirement; shutdown
waits for retirement and rejects worker reentrant close instead of joining itself.

The source ticket is the proposal ID. Retrying publication acknowledges the same stored
review, including its current correction or accepted state, without overwriting it.
After scope cancellation or restart, query Proposal or paged Proposals under current
access. Discovery returns source/engine/state summaries without transcription bodies,
works without a loaded model, and checks protection even for an empty page. Its auxiliary
SQL index is created only on authorised discovery; no logical schema/version change is
introduced. Correction, rejection and acceptance are explicit authoring commands.

Batch acceptance preflights all selections against the same pre-mutation source.
Distinct regions of one block can be accepted together; duplicate/overlapping whole-block
and region targets fail atomically. Each changed block advances once, and each affected
document receives its own transactional Undo journal entry. Original source observations
remain recoverable. Accepted proposal state records the historical acceptance even if
content is subsequently undone.

No SDK, model, weights or new network permission is introduced. Native test adapters
establish workflow behavior only. Android/JNI provider registration/execution, shared
capture scheduling, actual eligible runtime integration and isolated hard termination
remain open. Production H.I. remains gated.


### Observed-contact priority over recognition

Host.set_capture_priority accepts a trusted input-session ID and activity flag directly,
without JSON, SQLite or a writer wait. Beginning an observed contact cancels the prior
recognition epoch and retires idle source/runtime inputs on the optional cleanup lane.
New source admission is rejected while any contact remains active. Running adapters
receive ordinary JobControl cancellation; deliberately late success is rejected and
resources remain reserved until actual retirement. Ending capture permits newly
requested work only; it never revives a cancelled job or accepts output.

Session begin/end are idempotent, and ending one session never releases another. At
most 16 trusted session identities are retained. Overflow disables recognition for the
host lifetime rather than running optional work after losing a capture signal. It does
not close the authoring writer. Counts/closure/capture-active status contain no IDs or
content. This is a scheduling admission policy, not a CPU/RSS quota or hard preemption.

NativeBridge exposes the signal/status only to trusted application code. Android
JournaledCaptureSession signals observed pen/explicit-finger input, preserving default
finger navigation behavior. A contact's priority remains held through its final
checkpoint attempt. The private admission completion, rather than the caller-cancellable
receipt observation, controls retirement. A later observed frame owns the release generation: delayed pen-up
acknowledgement cannot resume optional work during the next contact. Independent
sessions, cancellation and lifecycle interruption retain their own boundaries. Signals
observed before native open are reconciled from current state after the handle becomes
ready, rather than replaying obsolete transitions. Full backend close disables native
recognition independently of checkpoint shutdown/recovery.

A priority signal is not a SQLite commit or durable journal receipt; existing receipts
remain authoritative. An unexpected signal failure is visible in content-free Android
recognition status and never discards original observations. No SDK callback or engine
cleanup runs on the input thread. Registration/execution of Android recogniser providers,
complete cross-media scheduling, hard worker isolation and physical-device performance
acceptance remain open; no model is introduced and production H.I. remains gated.


### Incremental encrypted whole-library backup (trusted core API)

Store.prepare_stream_backup returns generation/library-bound, read-only review metadata,
not a copied library snapshot. Existing explicit original-ink inclusion and protected
scope checks apply. Store.finish_stream_backup requires confirmation, fresh protected
proof and a JobControl. It reads generation-checked bounded SQLite pulls and streams fixed-table
logical records into EncryptStream; the sink receives ciphertext only. It checks the
current generation after each read snapshot, before authenticating completion, and
after final sink completion before acknowledging success.
Original stroke payloads/hashes are carried unchanged. Source caches are omitted;
authorised retained PDF bytes require explicit inclusion and export authority. Local
model-resource paths are stripped; engine metadata never grants runtime readiness.

STEMENC2 uses the already-resolved RustCrypto aead 0.5.2 STREAM BE32 implementation
with XChaCha20Poly1305, a fresh random data key and independent password/recovery-key
wraps. The existing password floor/Argon2id parameters remain unchanged. It uses 64 KiB
plaintext frames, bounded lengths and authenticated header/length/final-flag AAD.
STREAM maintains sequence/finality; the reader also requires exact underlying EOF.
Successful finish alone returns the offline recovery key. A failed writer/flush is
permanently poisoned; callers discard partial/unacknowledged encrypted output. Flush
is not completion. Callers still own file sync and deliberate atomic publication;
this generic byte-sink API does not claim an OS durability acknowledgement.

A decrypted authenticated prefix is not proof of a complete archive. DecryptStream
requires an explicit plaintext-byte budget; callers must consume EOF/finish before
using whole-stream results. Store.restore_stream_backup enforces that rule before its
private SQLite transaction commits. The logical STEMLIB2 body uses schema 15 and a
fixed compiled table order/column contract, typed bounded cells, table terminators and
an encrypted total-row footer. It never executes SQL supplied by archive content.
Individual cells are bounded to 64 MiB and the existing million-row admission bound
remains. These are engineering limits, not approved device SLOs.

Restore requires a caller-owned private directory (owner-only permissions on Unix;
symlink/shared-directory rejection occurs before creating private content). A random
create-new 0600 SQLite file stages the restore. The returned StagedBackupRestore owns
that file/connection and removes its own scratch on ordinary drop, closing SQLite
first. Failure returns no Store. The complete existing logical/foreign-key/identity/
source/history validator runs before returning a stage. Restore allocates a new library
identity, preserves protected scopes/original ink/history and retires platform grants
and runtime readiness through the same code as legacy restore. Publishing via
stage.store().persist_restored_library still requires fresh access/authentication and
explicit confirmation, syncs and atomically creates a new path without overwriting a
live library. The staged working DB is private plaintext, matching the established
working-database boundary; only encrypted output is a public backup writer.

Legacy STEMENC1/schema-8–15 backup APIs remain readable and retain their 64 MiB
in-memory limits. No native-subset, SQLite or receipt schema version changes. Tests
exercise an actual whole library above that old limit, plus large generated encrypted
streams, corruption/reordering/truncation/appending, cancellation, retained-source
consent, stale generation and no-overwrite publication. These are synthetic application
fixtures, not real-device measurements or a new cryptographic audit.

This is a trusted core streaming path, not Host/JNI/Android backup transport. Scheduling
and platform sink durability/cancellation integration remain open. The reused semantic
validator still materialises a complete logical graph; bounded validation, disk quota,
crash-orphan cleanup/review and comprehensive interruption/resource acceptance remain
open. Normal-drop cleanup is not a process-kill guarantee. Production H.I. remains gated.


### Bounded authoritative backup pulls

The exclusive SQLite host owner intentionally prevents a second read connection from
opening the live library. That ownership boundary is unchanged. StreamBackupCursor
provides the pull side needed for optional encryption/sink workers: each next_chunk
uses only the existing connection, copies at most 64 KiB, executes at most 64 cursor
steps, closes every statement/read transaction and returns. No sink, password,
encryptor or external callback executes within a pull. The cursor keeps logical position
and bounded single-cell engine-metadata scratch, not a live SQLite snapshot or borrowed
statement. Omitted source-asset tables/live-cache rows are skipped without reading their
blob payloads. Individual-cell limits remain; this is not a whole-process RSS guarantee.

Each pull checks the bound library/generation, exact-expiry fresh backup proof, supplied
current Access and JobControl before releasing bytes. It checks generation again after
the read transaction ends. Ordinary authoring may commit between pulls; the old cursor
then fails instead of finalising a mixed-generation export. Failed/cancelled/denied or
prematurely finalised cursors are permanently poisoned and retire their cached proof/
engine metadata. Later reauthentication cannot revive one. Some(empty) means yield;
only None means logical EOF. Consumers must successfully validate_complete under current
authority immediately before completing the encrypted stream and must acknowledge
success only after checking authority/source again following sink completion.

The existing finish_stream_backup convenience API now delegates to the same cursor and
retains source validation around encrypted finalisation. Wire envelope/body/schema and
legacy readers are unchanged. Cursor construction still performs deliberate fresh scoped
authentication; a cursor is move-only/non-deserialisable trusted native state, never a
JSON output, model input or permission grant to an optional adapter. The existing
exclusive-owner test additionally verifies a pull leaves the owner able to commit while
second connections remain denied. A blocked ciphertext sink test proves authoring can
commit before that sink is released; no tablet latency claim is inferred.

This core pull/consumer split is not yet a native Host job registry or JNI/Android backup
transport. Those still need bounded job admission, current host/capture epoch cancellation,
actual worker/sink retirement through close, trusted publication and platform durability.
Whole-graph validation, staging disk/crash cleanup and acceptance gaps remain open.


### Backup seek and access-capability checks

Backup row lookup now uses separate first-row, exact-rowid and next-rowid queries.
Exact/next queries use SQLite's rowid search; nullable OR predicates and class filters
no longer make one cursor step scan a table prefix or a run of omitted cache rows.
Live-cache rows still consume cursor steps, but CASE substitutes NULL for both their
bytes and following digest column. This avoids materialising the blob or fetching a
column beyond it while retaining metadata for the existing omission/authority checks.
Retained assets keep the unchanged column order and digest validation. An omitted
asset table is still skipped as a whole when source assets were not selected.

Access retains a private opaque allocation identity. Authentication creates a new
identity and relock changes it; private scope mutations must always do the same.
A cursor caches a successful required-scope check only for that exact allocation,
retaining a small Arc so allocator address reuse cannot alias a later capability.
The identity is not a permission grant and never crosses JSON/JNI or model interfaces.
The current capability must still be supplied. Clock expiry of both the private backup
proof and current capability is checked on every pull, independently of the identity.
App protection, source generation and cancellation checks remain per-pull. A new or
replaced grant triggers full scope validation; failure permanently poisons the cursor.
The first full validation polls cooperative control between groups of scope checks.

Tests use SQLite query plans and relative VM work on a fixed long-cache fixture to
verify rowid search rather than prefix/filter scanning. Separate tests expire an
unchanged capability, replace one at the same Rust variable/address, and relock a
cached capability. A real pull/encrypt/restore path over 130 live caches and an explicitly
retained snapshot verifies yielding, exclusion of cache marker bytes and retained-byte
fidelity. These are algorithm/security regression checks, not approved device SLOs.

The initial scope/metadata preparation and first validation still scale with library
size. Native Host job/worker lifecycle, capture scheduling and JNI/Android transport,
whole-graph restore memory, disk/crash cleanup and full acceptance remain open.


### Native host backup review, worker and retirement

Host.review_backup(options) returns a move-only BackupReview with read-only summary
metadata. It uses the current host access and binds the review to this host instance,
source generation, exact access expiry and shared capture/access epoch. One review/job
is admitted at a time. Metadata receives a conservative scope-count/input charge bounded
by the host query-byte limit. This is admission engineering, not measured process RSS.
A failed reservation/metadata budget leaves authoring available. Closed/stale/foreign
reviews cannot be used; a new review can reclaim cancelled idle metadata on its optional
caller thread. Close clears idle plan metadata even if a caller retains the old review.

Host.finish_backup consumes that review, explicit confirmation, a fresh trusted scoped
Authenticator, a JobControl and an owned Write sink. Call it on an optional worker.
The writer opens the private core cursor and answers bounded pull/validation requests;
password derivation, encryption and every sink write/flush/destructor execute on the
caller worker. Ordinary JSON cannot create any of these capabilities, supply a sink,
claim readiness or bypass protected review. Authentication remains the existing trusted
platform callback, not model code. Current access must already cover protected review
scope; fresh backup verification does not silently grant wider ordinary editing access.

Backup work shares the native recognition capture/access epoch. Capture start, relock,
authentication attempts, successful protection/containment/history/deletion changes,
access expiry and host close cancel old authority; pen-up/new authentication never
revive it. Ordinary edits instead invalidate the bound generation. Caller deadline/
cancellation and any existing JobControl parents remain linked. Source/authority checks
surround encrypted completion, sink retirement and final acknowledgement. A blocked or
non-cooperative sink is not claimed terminated by logical cancellation.

The owned sink never escapes in the result. It is dropped before BackupReceipt can
expose its Zeroizing recovery key; the receipt has no Debug/Serialize implementation.
Admitted sink write/flush panics are caught before the sink is separately retired under
a panic boundary. Destructor panic/failure, stale source, cancelled authority or cleanup
failure yields no success receipt. Reentrant close from a sink callback returns an
error instead of self-joining. Authoring remains usable after ordinary optional failures.

Close stops admission, drains accepted authoring and clears the actor cursor, then
retains exclusive library ownership until actual backup worker/sink retirement. Private
metadata/cursor/worker guards release the single job only after their owned state is
retired. A saturated ordinary query budget cannot strand cursor cleanup: one fixed
lifecycle-only Release slot is reserved. It runs after authoring and before ordinary
reads, may add one to pending_queries, carries no content bytes and cannot admit a
user request. Ordinary data-query budgets and authoring FIFO remain unchanged.

A successful receipt acknowledges a complete encrypted logical SQLite stream and owned
sink retirement. Generic Write flush/close is not an OS file-sync/atomic-publication
acknowledgement. The committed SQLite library is the dataset; private active/legacy
capture recovery stores are separate. JNI/Android must review pending drafts and bind
trusted output capabilities, quota, sync and publication before an app-level backup
success. Those workflows are not implemented by these Rust-only endpoints.

Tests cover protected review/fresh proof/confirmation, foreign and retained closed-host
reviews, actual above-64-MiB native export/restore, original ink, generation/capture
cancellation, blocked sink authoring, metadata rejection, callback/destructor panics,
cleanup with saturated queries and exclusive ownership held during blocked write and
post-encryption destructor retirement. No device latency or hard-termination claim.
Initial metadata/proof preparation remains proportional to library size. JNI/Android
transport, broader resource scheduling, platform durability, bounded semantic restore
validation and disk/crash cleanup remain open; production H.I. remains gated.


### Trusted JNI backup transport

NativeBridge.reviewBackup(handle, originalInk, sourceAssets) returns a typed,
AutoCloseable BackupReview bound to that exact non-reused host identity. The
summary describes actual included ink/source assets and omissions; no library
payload or recovery key crosses ordinary JSON. begin(budgetMillis) transfers the
review into a single-use BackupJob. Its 1..3,600,000 ms input bound is an engineering
limit, not a device latency promise or extension of protected access expiry.

Run the job on a dedicated optional worker, never NotebookBackend's authoring
executor. run(passwordChars, freshVerification, confirmed, output) consumes the
job, revalidates native authority and sends only bounded ciphertext frames
(maximum 65,552 bytes) to the trusted BackupOutput. Password input is bounded
before Java cloning and JNI allocation; temporary Java and Rust copies are
cleared, including malformed UTF-16 decode failure. The caller retains and clears
its original password. Success returns a caller-owned recovery-key char array;
clear it after the deliberately reviewed recovery workflow. It is never a String,
JSON field, logged diagnostic or implicit clipboard value.

BackupOutput.retire must synchronously retire every owned resource before
returning, even if it reports failure. Native sink destruction calls retire while
the admitted core worker remains counted. The Java wrapper makes exactly one
retirement attempt and supplies fallback retirement when argument/admission
rejection happens before native sink construction. Write/flush/retirement and
verifier exceptions become generic errors; retirement failure suppresses key
exposure. No hard termination or ownership of external resource clones is
claimed. close() cancels logical work without releasing a running registry entry;
actual native completion retires the entry. Host close removes admission before
cancelling jobs, and no host/job registry lock spans Java callbacks or drain.
Reentrant close retains native self-join protection.

These trusted endpoints are not yet NotebookBackend's app-level backup workflow.
They cover committed SQLite content only. A future controller must own and await
all optional workers (including work rejected before native admission), reconcile
private active/legacy capture journals, and bind reviewed destination, file sync,
quota and atomic publication before acknowledging a durable backup. Core close
only counts admitted core work; callers must retain Android directory ownership
through their own workers' entire lifetimes. JNI restore, scheduled/public-provider
transport, bounded semantic restore validation and crash cleanup remain open.
Production H.I. remains gated. Actual JVM checks and API-35 Robolectric tests are
host evidence, not device or provider durability evidence.


### JNI consumer rules and release packaging

The backend library packages consumer-rules.pro into both debug and release AARs.
The rules retain NativeBridge exported names, JNI-created nested constructors,
callback interface members and concrete Verification/PdfGrant/BackupOutput methods
called only by native code. They do not disable shrinking for unrelated backend
code. No Activity or production H.I. is introduced.

The Android validator now builds both AAR variants, compares their consumer rules
against the tracked file, verifies identical ARM64 libraries, and checks privacy
merge contracts in both packaged manifests. It compiles fresh JVM classes in a
private temporary build directory, runs the complete native smoke boundary, then
uses the installed SDK 35 build-tools R8 in release classfile mode with tree shaking,
optimization and minification enabled. Only the fixture main entry point is kept
by test rules; the actual library consumer rules retain JNI names. Mapping inspection
requires BackupBoundarySmoke to have been renamed. The same -Xcheck:jni smoke groups
must pass in the optimized artifact. Both concrete verifier classes and lambdas,
PDF grant callbacks, backup review construction/output/retirement, Unicode,
protection and close ownership are exercised. Tool version/hash, input/output hashes,
mapping and raw command output are retained in evidence. JNI group totals are read
from smoke output instead of the old fixed jni_assertions field.

This is actual desktop R8/JVM execution plus debug/release AAR packaging evidence,
not final APK/ART, device authentication/privacy or a shrunk production app test.
The acceptance gate requires these new checks without closing the broader Android
boundary/semantic gaps. App-level backup/recovery integration, worker ownership,
trusted quota/sync/publication and JNI restore still remain open; production H.I.
remains gated.


### Android local backup controller

NotebookBackend.backups() exposes a trusted NotebookBackups controller, not a JSON
command or model capability. One review/artifact and one optional operation are
retained. A dedicated daemon worker performs native review, encryption, staged file
I/O, copy/sync/publication and cleanup. The bounded queue admits one ordinary task
plus one lifecycle cleanup task while a previous task finishes. Encrypted payload I/O and native backup job waiting stay off BackendWriter;
bounded capture-folder metadata and generation barriers are serialized there. Pending.await is for an observer
thread. Cancellation is logical; closeFullyAsync awaits actual optional workers and
cleanup before releasing LibraryOwnership, including pre-native and post-native work.

Review/create use serialized authoring-writer barriers: initial journal recovery
must finish, both private active/legacy capture directories must be empty (a bounded first-entry
directory-stream check includes unknown/incomplete entries), no observed contact or failed priority signal may be
active, and the non-reused local epoch and exact native SQLite generation must
match. The epoch changes before authoring/access admissions and observed contact
updates. NativeBridge.backupGeneration returns exact decimal metadata behind the
existing app-authorised Status check, avoiding Java JSON floating-point precision
loss. Content-free FailureReason codes distinguish recovery, stale source, quota, staging,
integrity and destination-exists failures without propagating callback diagnostics.
A blocked or corrupt/private capture record is retained for explicit recovery;
backup never disposes drafts or reports they were included in committed SQLite.

A reviewed create requires confirmation, caller-owned bounded password chars, fresh
platform verification and explicit maximum encrypted bytes. It creates an owner-only
encrypted private staging file; native sink retirement forces and closes its channel
before key delivery. Temporary password/key buffers are cleared on failure,
cancellation and retirement. Prepared.copyRecoveryKey is an explicit trusted copy;
its caller must clear the returned char array. Closing/invalidating the prepared
artifact clears its managed key and serializes deletion behind all file work. No
unguarded reader or public path is exposed by this API. Staging alone is not a
Backed up acknowledgement. Existing staging/orphan/unknown files block a new create
and remain for recovery; no automatic sweep or crash-cleanup completion is claimed.

Prepared.publish chooses a trusted absolute local file outside managed no-backup
storage and requires explicit confirmation. It copies only ciphertext to a new
random file in the selected parent, enforces the original size/digest, forces/closes
the copied file, publishes using an atomic no-overwrite hard link, syncs the parent,
unlinks the temporary and syncs again. Existing destinations and symlinks are rejected.
The filesystem must support hard links and directory sync; unsupported destinations
fail without replacing a file. This local-file capability is not a SAF/provider grant.

Publication reports Published only after these filesystem acknowledgements. Failure
after the link reports Uncertain, retains credentials in the still-owned prepared
artifact and leaves any final encrypted file for explicit inspection rather than
removing it. Cancellation after entry to publication commit returns false and drains
to an outcome; close retains ownership through the drain. The receipt's current flag
is a point-in-time epoch/generation comparison, not a promise to cover later edits.
A valid older snapshot may be published with current=false. Authentication,
protection/containment/deletion policy changes invalidate uncommitted reviews,
artifacts and key capabilities; an already committing publication drains to its
explicit outcome. No operation can revoke a recovery-key copy deliberately taken
by the caller earlier.

Tests exercise actual JNI, original source-ink preservation, private and unknown
recovery barriers, protected capture replay, stale review/current state, quota and
sink failures, digest/no-overwrite checks, relock, uncertain publication and ownership
through blocked pre-native/sink/post-native/publication workers. These are host
API-35 Robolectric/local filesystem checks, not device/provider guarantees.
SAF/provider transport, scheduling, persisted backup catalogue/status, JNI restore, explicit orphan recovery/cleanup,
full resource scheduling and bounded semantic restore validation remain open, along
with other registered backend gaps. No production H.I. is introduced or authorised.


### Reviewed Android provider backup publication

Prepared.reviewProvider(treeUri, pickerFlags) reviews an explicitly selected SAF tree
for that exact owned encrypted artifact. Read/write picker flags are necessary but
are not treated as permission proof. The production BackupDocuments adapter checks
current OS URI access, queries the selected directory identity/type/create support
and label, and binds a bounded selection capability. Selection/payload/provider work
runs on the optional worker. No URI grant or backup credential comes from notebook
JSON/model code, and no grant is silently persisted for scheduled use.

Publishing requires confirmation and fresh destination/grant observation. The
controller verifies staged ciphertext before external creation, creates a fresh
randomly named octet-stream document through DocumentsContract, checks the returned
tree/authority/document identity and child relationship, then writes only bounded
ciphertext. It never accepts an arbitrary existing output URI or requests overwrite
of an existing selected document. This relies on the selected provider honouring
Android's new-document contract; it does not establish provider honesty.

Write descriptors/streams are closed before reopening for an exact bounded length/
SHA-256 readback. Reliable descriptor errors and close failures suppress verification.
Descriptor sync is attempted; unsupported sync does not by itself prevent verified
readback. The result records whether that call was acknowledged. ProviderVerified
means the immediate readback matched the prepared encrypted bytes, not local atomic
publication, remote commit, durable cloud storage or two-way sync. Production app
code must preserve that distinction. The destination receipt exposes the deliberately
created URI to its trusted caller and has no generated toString containing it.
Neither passwords nor recovery keys enter the provider adapter.

Errors/cancellation after external creation begins return Uncertain, possibly with
no known document URI if creation itself was ambiguous. No remote document is
silently deleted or retried; credentials remain in the still-owned prepared artifact
for explicit recovery until it is closed/invalidated. Local-file commit retains its
existing non-reversible cancellation boundary; provider cancellation instead requests
stop and drains to a verified/uncertain outcome after resources retire.

A per-operation CancellationSignal and remaining-deadline timer cover provider
metadata/open/read/write waits. Signal callbacks run on a separate bounded cancellation
worker. Its slot remains occupied and new ordinary operations are rejected until a
callback actually retires. Close awaits payload, cancellation and timer workers before
library ownership can be released. No blocked provider is claimed forcibly terminated.
Source-policy invalidation retires the artifact/destination capability; current-source
flags remain point-in-time epoch/native-generation comparisons.

Actual JNI/API-35 host tests use synthetic provider adapters and descriptors. They
cover live/revoked grants, fresh selection, independent new documents, ciphertext
integrity, readback mismatch, ambiguous creation, non-child results, cancellation/
timeout and actual callback retirement, authoring while readback waits, relock and
simulated non-seekable output with unsupported-sync injection. Robolectric's pipe
and sync behaviour is not ART/kernel or cloud-provider evidence. The production
adapter's OS-denied check is exercised separately. No production H.I. is added.

Source contract: [Android DocumentsContract](https://developer.android.com/reference/android/provider/DocumentsContract).
Scheduling/persisted grant consent, saved/backed-up catalogue, JNI/platform restore,
orphan recovery, device/real-provider acceptance and other backend gaps remain open.


### Native restore review and recovered-copy ownership

Host.prepare_restore owns an admitted reader on the trusted optional caller worker.
It obtains current app-authorised lifecycle authority through one bounded writer
metadata message, then reads/decrypts/validates off the live writer. Both STEMENC2
streaming schema-15 and bounded STEMENC1 legacy envelopes are accepted through the
existing validators/migrations. Caller plaintext budget bounds legacy envelope
admission before decrypt allocation; the old 64 MiB limit remains only on the
legacy format, not a total-size limit on current streams.

One host-local move-only RestoreReview owns a private SQLite stage and aggregate
counts (entities, blocks, original strokes, references/assets, protected scopes).
It exposes no Store, raw payload or private path. The stage directory must be a
real owner-only directory on Unix; random scratch files are create-new/0600.
Authentication/runtime resource and source-grant retirement follow existing core
restore rules. A protected recovered library stays protected. Review metadata is
bounded by the host admission budget; semantic graph validation still has the
separate unresolved whole-graph memory limitation recorded in A6.

A reader is retired before review acknowledgement. Read panics and reader-destructor
panics are contained separately, and actual retirement keeps the worker/job counted.
Capture, access/lifecycle expiry, cancellation, deadline and host close invalidate
optional authority without reviving it at pen-up. Linked controls preserve all
parent cancellation/deadline/expiry constraints. Rejected/unadmitted resources still
belong to their caller's outer lifecycle, as in the backup/JNI boundaries.

Host.publish_restore consumes the review, explicit confirmation and a fresh trusted
platform authenticator. Verification runs on the restore worker, including for an
unprotected archive; it does not grant wider access to the live library. One proof
covers the protected recovered scopes plus App. The destination is a separate
owner-only private directory. A private 0600 temporary copy is synced and published
by an atomic no-overwrite hard link inside that directory, with directory sync and
temporary retirement. Existing live or unrelated files cannot be replaced.
Published acknowledges those local steps; any error/cancellation after the final
link is Uncertain and leaves the recovered copy for inspection. The caller-selected
path is known to the trusted caller, not an ordinary JSON/model grant.

Close stops admission and invalidates authority, clears idle private stages even if
a caller retains review metadata, and keeps exclusive live SQLite ownership until
actual restore readers, stages, verifiers and cleanup retire. Reentrant close from
reader/destructor/verifier callbacks returns an error instead of self-joining, while
still closing admission. A saturated/blocked optional reader never runs on the live
writer; independent authoring can proceed. No hard termination is claimed.

Core tests exercise both formats/credential paths and exact original ink, confirmation
and one fresh proof, wrong/tampered/truncated/trailing input, budgets, one-slot and
retained-review close, reader/destructor retirement, capture invalidation, separate
panic boundaries, reentrant close, authoring during verification, existing/foreign
and unsafe/symlink destinations, a real above-64-MiB host restore, exact authority
expiry and post-link uncertainty. Resource-heavy restore fixtures are serialized
inside their integration test binary; timings are host observations, not device SLOs.

These native trusted APIs now also have the JNI transport described below;
Android worker/provider ownership and explicit recovered-root opening are
implemented below; production H.I. remains gated. Reviewed orphan cleanup/quarantine is implemented below; hard quota workflows
remain unfinished. Provider/device acceptance, bounded semantic
validation, crash cleanup and the other backend gaps remain open. Production H.I.
remains gated.


### Trusted JNI recovered-copy transport

`NativeBridge.beginRestore` reserves one host-bound, non-reused job token with a
bounded cooperative deadline. `RestoreJob.prepare` consumes ciphertext from an
owned `RestoreInput`, copies the caller-owned credential into bounded zero-cleared
storage, and returns only count metadata. Password and recovery-key modes share
the existing current/legacy core decoder and semantic validation. Neither source
content nor a stage path is returned. Caller input/credential validation failures
also run exactly-once retirement through the Java wrapper. A callback exception,
zero/oversized read result, or failed retirement suppresses review exposure and
is reported with a generic diagnostic. Read callback arrays are at most 65,552
bytes; this is an engineering bound, not a performance target.

`RestoreJob.publish` consumes the stage after fresh trusted proof and explicit
confirmation, to a separate absent app-private destination. It returns the JSON
publication state (`"Published"` or `"Uncertain"`) without activating the copy.
Closing the job cancels running work but keeps the token reserved until actual
reader/stage retirement; idle stage cleanup occurs outside the global registry
mutex. Foreign-host tokens are rejected before consumption. Closing the native
host cancels its restore jobs and retains the core's draining ownership contract.
App-owned pre-admission and post-native input/grant work still needs Android
controller ownership through `closeFullyAsync` before releasing library ownership.

The desktop `RestoreBoundarySmoke` fixture covers actual JVM/JNI callback
roundtrip/publication, foreign-token rejection, failure redaction, bounded reads,
failed retirement, pre-native rejection, blocked-read cancellation/admission and
idle-review host close. It joins the normal and optimized R8/JVM runner; consumer
rules retain `RestoreInput` concrete callback members. Running the fixture on a
desktop does not establish ART, provider, disk-quota, crash cleanup or device
acceptance. A poisoned/corrupt runtime or uncooperative input is not claimed to be
hard-terminated. Trusted private paths and all input resource clones remain the
app transport's responsibility.

### Android recovered-copy controller and explicit library opening

`NotebookBackend.restores()` owns one bounded optional restore worker and one
review. A deliberate read picker supplies the URI and returned read flag;
`RestoreDocuments` independently checks current OS read permission, opens a
read-only descriptor and retires it through the Java/native input wrapper. No
persistable grant is taken automatically. Permission is rechecked at reads,
after staging and before publication. Credentials are copied into bounded,
zero-cleared storage; the caller still owns and clears its original array.

`prepare` takes caller-selected ciphertext/plaintext bounds and a minimum free
space reserve. The controller checks input bytes and free space on admission,
while reading and after native validation. These checks are not a hard SQLite
staging disk quota: native allocation/whole-graph validation and crash-orphan
cleanup remain open. Stage directories are mode 0700. Unknown/nonempty orphan
staging blocks new admission and sets `needsAttention`; it is never swept as a
cache. Normal retirement deletes only the exact empty owned stage directory.

`Review.publish` requires explicit confirmation and fresh platform verification,
creates a distinct app-private recovered root, and preserves `Published` versus
`Uncertain`. It never replaces the working database or consumes current capture
journals. The root and its bounded catalogue marker are synced; a marker update
or post-publication acknowledgement failure preserves an uncertain receipt and
the possible copy. Catalogue discovery is paged with bounded retained entries,
validates bounded markers without following symlinks, and exposes no titles or
content. Snapshot counts remain historical after the recovered copy is edited.

A returned or rediscovered `RecoveredLibrary` is a trusted capability, not an
ordinary JSON path. `open(context)` is an explicit separate action that validates
its marker and existing SQLite file before creating an independent backend. A
missing copy cannot silently become an empty library. Restored protections and
original ink remain in the recovered database; original runtime/source grants do
not become new access. This is backend integration, with no Activity or production
H.I. Storage corruption and device durability still require broader acceptance.

Cancellation asks the native job and provider signal to stop on a separate
bounded cancellation worker. Admission and `closeFullyAsync` retain ownership
through actual pre-native open, read/close, post-native work, cancellation listener
and private stage cleanup. No blocked callback is claimed hard-terminated.
`Pending.await` waits for terminal retirement, including after cancellation;
thread interruption can stop observation while work remains owned. Public cancel
returns false after publication entry so a possible publication receipt is not
lost. Lifecycle close/relock may still request cancellation and retain an honest
uncertain result. Authentication and permission/containment policy changes
invalidate private reviews, with stage retirement off the authoring/control thread.

`NotebookRestoresTest` uses actual desktop JNI in Robolectric API 35. It covers
independent ink/protection recovery with password/key paths, explicit opening and
restart discovery, live draft preservation, quota and malformed input rejection,
failed retirement, revoked grants, blocked read/open/cancellation-listener close,
relock, off-writer cleanup and ambiguous publication. These are synthetic provider
and filesystem fixtures, not ART, real provider or device-power-loss evidence.

Recovered activation uses `Host.open_existing` / `NativeBridge.openExisting` after
app LibraryOwnership admission. SQLite opens without CREATE and checks the stored
supported schema, baseline notebook tables, generation/protection settings and
library identity before migration/write admission. It rejects missing, empty,
foreign or invalid-identity databases instead of initializing them. Existing
legacy notebook schemas still migrate through the normal transactional path.
No Java/raw database descriptor is opened for preflight: closing an unrelated raw
FD could otherwise disturb this process's POSIX SQLite locks. Duplicate-open tests
verify that both app and native rejected duplicates leave the original writer's
exclusive ownership and subsequent edits intact.

### Reviewed unfinished restore cleanup and offline guidance

New restore preparations persist a purpose/library/stage marker before native
work. `NotebookRestores.reviewStages` retains one host-bound native reservation and
returns a bounded page of opaque, epoch-bound items/cursors. It cannot inspect a
currently owned JNI restore stage. The same optional worker owns inspection,
proof/confirmation, transfer and actual cleanup through full app close.

`StageReview.retire` consumes a selected item with `DiscardRecognised` or
`Quarantine`. Current app access and a fresh platform verifier are required; stale
metadata or marker digests refuse mutation with `StagingChanged`. Identified private
scratch, verified protocol-only records and exactly empty private recovery folders
may be discarded. Unclassified content is preserved in a separate private retired
container and cannot be discarded or followed through links. This unblocks new
restores when pending staging is cleared without silently sweeping unknown bytes.

Both namespace roots are bound to their original filesystem identity. Files are
checked by device/inode, UID/mode, link count, size and nanosecond-capable modification/
change-time observations; marker JSON alone is read/digested. Raw SQLite payloads
are never opened for inspection. Existing working SQLite ownership is therefore
independent of orphan inspection, including a hardlinked-file negative fixture.

Transfer happens only into a freshly reserved private container. After commit
entry, a possible-effect receipt is preserved: `Discarded`, `Quarantined`, or
`Uncertain`, with the retirement ID and acknowledged file bytes. Lifecycle close
can stop further work cooperatively but cannot turn possible file changes into a
lost cancellation result. Restart discovery can finish identified interrupted
retirement; unknown retained data remains preserved. No secure-erasure, total disk
quota, complete content hash or real-device durability guarantee is implied.
See `docs/formats/RESTORE_STAGES.md` for the on-disk protocol and limits.

Generic offline guidance is available as the read-only host command
`{"op":"Help","topic":"RestoreCleanup"}` and as `NativeBridge.help("RestoreCleanup")`
without opening any library. It explains permanent discard, separate current-library
content, preserved quarantine and uncertain outcomes. Static guidance remains
available while app content is locked and does not invalidate a live review or
change library generation. No network/LLM or internal-log knowledge is needed.

`RestoreStagesTest`, `RestoreStageLifecycleTest` and the existing
`NotebookRestoresTest` cover these contracts with host filesystems, synthetic
providers and actual desktop JNI. `RestoreStageCrashProbe` deliberately exits a
separate JVM after native staging without destructors; it is a developer fixture,
not Android process-kill or physical-power-loss acceptance. Host Unix metadata
coverage does not certify the Android lstat fallback or device filesystem.

### SQLite-backed restore validation update

The earlier whole-graph Rust allocation limitation is superseded for semantic
validation: current whole-library restore traverses rows and indexed SQLite work
state, independently of subset-export caps. Both legacy and streaming paths keep
schema and semantic checks. Both streaming and legacy worker restore supply their lifecycle JobControl
through row traversal, edit-journal validation and incremental image/source
hashing. Legacy ingestion also polls between rows; individual decryption/JSON or
SQLite calls are not forcibly interrupted.
Private validation tables/indexes are rolled back before review; no capability
or extra schema is exposed. The encrypted input/cell and per-record domain limits
remain. This is not a total native memory, SQLite/pager or scratch/WAL quota, nor
forced cancellation within SQLite calls. Complete native allocation budgeting and
device/storage interruption acceptance remain open. See full-library-backup-v1.md.

Embed traversal uses incrementally inserted private `(host,edge)` primary-key
work records instead of creating an index over the populated source table. The
pinned SQLite query-plan fixture checks covered discovery, bounded next-edge
search, no temporary B-tree sorting and negative/zero edge ordering.

Post-validation source-grant retirement now uses bounded rowid traversal and the
same cooperative control, preserving revoked references and committing fresh
recovered identity atomically with availability retirement. Cancellation rolls
back an already-updated prefix; neither source rows nor source grants escape
through a review of an incomplete stage.

The open restore stage no longer receives a second raw database descriptor for
fsync. FULL synchronous SQLite commits and a checked truncating checkpoint own
sync/locking; busy or non-WAL/no-op results fail before review. Publication still
syncs its closed new-copy file and directory before acknowledging that copy.

### Persistent backup publication status

`NotebookBackups.status()` and bounded `history(before,limit)` use the existing
optional worker and fresh native app authority before/after reads. A separate
private SQLite catalogue records the actual prepared library/generation, selected
external destination, ciphertext count/hash, acknowledgement kind/sync, raw-ink
inclusion, retained source count and omission facts after successful publication.
It neither changes notebook generation nor enters a portable backup. Source
metadata queries account for active/pending/recovery/failed authoring before
claiming all current authoring was published. Catalogue reads do not contact an
external destination, and historical locations are not URI permissions.

Local/provider receipts now carry `catalogueRecorded` separately. A history-write
failure preserves the known file outcome, leaves previous records intact and sets
backup attention; a possible/uncertain file operation never becomes a successful
history row. FULL synchronous metadata transactions and directory sync stay within
the worker lifetime. Close retains the app lease through blocked catalogue work.
Unknown/future schemas, file links, redirected namespaces and corrupt records are
preserved and reported unavailable. See BACKUP_PUBLICATION_HISTORY.md and ADR 007.

### Native shared engine controller

`Host::engine_activation_plan(version)` captures stored consent/version/generation metadata and host optional-work authority. On an optional worker, call the plan's typed preparation method with the SDK adapter and resource bound, then `Host::install_prepared_engine(&prepared)` to publish to the private host Manager. Drop the prepared holder after success/failure so old/abandoned SDK ownership retires off the writer. `Host::prepare_runtime_recognition(block,region,task,override_id)` prepares owned work against that manager; run it on an optional worker and publish its completed proposal through the existing revision-safe review path. Explicit override is supported; installed metadata after reopen is not runtime readiness.

Close disables new private manager leases and waits for their actual retirement before library ownership is released, including SDK destructors. Both registry and optional-work retirement reject reentrant self-waits. JNI typed provider registration and Android controllers still require implementation; no model is enabled by these native APIs.

SDKs that consume a staged model/configuration should use the plan's `prepare_recogniser_with` / `prepare_converter_with` methods. Their worker factory receives hash-verified bounded bytes and the exact typed descriptor; it receives no file path or live Manager. Complete loading or retain a protected copy before returning, since the temporary core buffer is zero-cleared. Factory/probe cancellation and failure keep the previous live runtime and installed pointer unchanged. Preconstructed-adapter methods remain for trusted resource-free/already-owned integrations.

### Trusted Java recogniser providers

`NativeBridge.stageResourceFreeEngine` records explicit consent for an app-compiled adapter without staged model bytes. `engineActivation(host,version,maxLoadBytes,budgetMillis)` returns an AutoCloseable single-use EngineJob. Its `activate(factory)` constructs/probes the adapter using the exact stored plan; run it on a dedicated optional worker. `engineRecognition(host,block,region,task,engine,budgetMillis)` captures authorised source and `recognise()` returns a durable pending proposal. Close cancels the job; native Host close still waits for actual inference/SDK retirement before releasing the library. No Manager/Store capability crosses Java.

Factories implement partial-construction cleanup; returned Recognisers implement probe, ordered-ink recognise and retire. Resource arrays are temporary and cleared after create; copy/load before returning. Callback EngineControl expires after the callback and polls the exact native authority/deadline/runtime state. SDK callbacks must cooperate with it. Provider output/errors do not author source. R8 retains native constructors and concrete callback methods; actual optimized JVM fixtures supplement, rather than replace, Android/device acceptance. Android resource-backed import/controller and eligible real-model integration remain open.

### Android optional engine controller

`NotebookBackend.engines()` exposes the shared backend's bounded optional lane. `stageResourceFree`, `activate` and `recognise` return Pending immutable receipts; SDK callbacks run independently of the authoring executor. Observe Pending.await on an observer worker, not the main thread or the controller's own worker. Pending.cancel/close request cooperative cancellation, while known successful receipts remain observable. Failure.metadataMayHaveChanged explicitly preserves uncertainty after a native attempt instead of implying rollback. Proposal receipts contain IDs/revision; read/accept reviews through current native authority.

Backend close cancels/drains this lane and native SDK retirement before releasing the Android library lease, including failed-open cleanup. No engine is enabled automatically. Resource-backed Android import, catalogue/default/uninstall workflows, actual model integration and hard SDK isolation are still open.

### Native resource-backed import

`Host::engine_import_plan(descriptor,total,consented)` reserves metadata only. On an optional worker, start it with an app-private backup-excluded model root and optional resume ID. Retain the stable transfer ID and inspect committed_bytes before opening/seeking the selected input. `HostEngineImportTransfer::prepare` takes an owned reader positioned at that durable offset, copies/verifies it and retires the reader before returning a private prepared holder. `Host::commit_engine_import(&prepared)` publishes only metadata on the writer. Failed/cancelled preparation/commit preserves explicit resume state; it does not promise no filesystem effects or automatic discard.

Binding/manifest checks precede resumed truncation. Bound consent is recorded in the private ledger. Original app expiry and capture/access cancellation surround chunks/hashes and the transaction. Repeated acknowledged identical transfer IDs are idempotent; removal is not reversed. Future activation still performs its own verification/probe; staged/installed metadata is never inferred readiness. JNI/Android import/controller and actual-model acceptance remain separate unfinished work.

### JNI/Android model-resource input

`NativeBridge.engineImport` returns a single-use EngineImport with stable version/committedBytes before input opens. Its copy factory opens inside native running admission and receives exact linked EngineControl; the returned EngineInput reads bounded buffers and retires its actual handles before metadata publication. Buffer references retained by an input/factory see cleared arrays after callback completion. Native close waits for admitted opening/read/cleanup; ordinary JSON has no input/path/SDK capability.

`NotebookEngines.importResource` owns the private no-backup model depot, reports Transfer before input opens, and supports explicit resume IDs. EngineModelSource.fromPicker observes a content/read selection; actual opening happens on the optional worker, uses r mode and seeks from the durable offset. It does not persist a URI grant automatically. Failure may leave resumable files, and fresh source selection may be required after restart/revocation. Catalogue/discovery/default/uninstall/discard and actual-model/device acceptance remain unfinished.
