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
