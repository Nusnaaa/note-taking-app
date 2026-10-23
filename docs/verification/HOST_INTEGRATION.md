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

`PdfWorkerService` owns each incoming descriptor through admission, rejects busy work
without leaking the descriptor, validates render bounds inside that ownership scope,
and closes both pipe ends when reply/compression fails. The isolated parser receives
no notebook path or access tokens and never determines retention authority. Pixel pipes
must be fully decoded/validated by the client: an `ok` reply alone is not rendered-image
success. Complete client binding, persisted source-policy synchronisation, parser
process timeout/revocation orchestration, annotation/navigation and device execution
remain open. The Android module explicitly pins Build Tools 35.0.0.

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
