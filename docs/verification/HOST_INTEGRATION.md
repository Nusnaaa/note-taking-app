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

The Kotlin executor is still a single queue: end-to-end Android capture priority and
status observation remain incomplete. Encoded-byte limits do not claim a total RSS bound.

The CLI has no authentication bypass. This optional developer harness does not replace
or require the owner's deferred handwriting/model tests on a complete runnable app.

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
