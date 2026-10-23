# PDF lifecycle increment — 23 October 2026

Source checkpoint: `8e8e16319420941b02b844a359972b287bd80393` on `phase-a/backend`, following the validated native
host admission checkpoint. Backend acceptance remains NOT MET; production H.I.
remains gated. This increment does not complete A5-PDF-INTEGRATION.

`PdfProvider` opens read-only content-provider descriptors on bounded background
workers, with a bounded pending set, deadlines and cancellation signals. Cancelled
or timed-out provider calls retain their slot until the provider actually returns;
ready but unclaimed descriptors also retain their slot. Late results close. A
separate bounded cancellation executor prevents a blocking cancellation listener
from occupying the deadline thread. Provider close retires unclaimed descriptors.
Only `PendingPdf.await` transfers descriptor ownership; closing the pending handle
after transfer does not close the caller-owned descriptor. URI scheme/length and
opening deadline ranges are validated before admission. Errors omit provider details.

The isolated PDF service owns incoming descriptors through admission and parsing,
including busy rejection, invalid dimensions and failure to post to the parser
thread. Pixel streaming closes both local pipe ends on failed Binder publication
or compression. The local reader closes before writing so a disconnected remote
reader cannot be masked by a reader retained in the worker. The client must validate
complete image bytes, not equate a successful pipe publication with successful
rendering. The parser control looper remains separate from parsing/pipe writing.

Ten new Android regression methods cover successful off-thread ownership transfer,
uncancellable-provider timeout/backpressure and late cleanup, a blocking cancellation
listener with independent deadlines, abandoned ready results and provider close,
content-free failures, grant revocation, explicit cancellation, malformed acquisition,
invalid-dimension input closure and both-pipe-end closure on failed reply. They use
synthetic URIs/temporary descriptors and tiny generated bitmap fixtures. They are
Robolectric component checks, not device/provider-vendor or real Binder isolation
acceptance. The current complete test suite has twenty Android methods.

The preliminary scratch run passed nineteen tests/lint; after adding the explicit
cancellation fixture, the worktree run passed twenty tests/lint. A final URI-length
bound and its malformed-input assertion were added before the source checkpoint;
the full exact-source validation ran after that checkpoint. Format, strict workspace
Clippy, all 138 Rust tests (no failed/ignored), release build, traceability, ARM64/ELF,
all seven actual desktop JNI smoke groups, all twenty Android unit/Robolectric tests
(no failures/errors/skips), lint, packaged ARM64 AAR payload and merged library
manifest policy passed. Desktop and Android input fingerprints match, with no
uncommitted non-evidence inputs. Observed commands, results and fingerprints are
recorded under `continuation-2026-10-23/pdf-lifecycle/`. The backend gate still exits 2
on substantive open gaps and incomplete canonical/inherited-clause coverage.

Build Tools 35.0.0 is now explicitly selected, matching the documented toolchain.
The earlier concurrent scratch build's implicit Build Tools 34 auto-install race
was repaired from the upstream checksummed archive; failed preparation is not
counted as successful application validation. A sandboxed Gradle attempt also
failed because daemon socket creation is prohibited; the actual tests run outside
that sandbox with cached tooling. Neither event is hidden as a passing test.

Traceability now includes executable Kotlin fixture references for this PDF group,
in addition to the existing Rust references. All 94 requirements, 103 inherited
clauses and 158 historical candidate assertions remain preserved, and none is marked
complete merely because these component checks pass.

Still required: actual parser client binding, generation/revision-bound persisted
source-policy checks, parser-process timeout/termination and revocation, full PDF
navigation/annotation workflows and the remaining service/device acceptance. An
opened descriptor does not supply authority to keep/export a source. No notebook
path or authentication token is passed into the isolated parser. No managed copy,
handwriting upload, remote inference or production H.I. is introduced.
