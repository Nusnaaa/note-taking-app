# Current I0+I1 implementation and verification map

The three generated CSVs preserve all **94 canonical requirements**, **103 inherited
clauses** and **158 historical candidate rows**. The last number is not an executed
test count. `tools/traceability.py` checks row retention, explicit current-phase
mapping, source-clause presence and real executable references. It does not certify
semantic completion. `validation.json` records observed execution separately.

Requirements map to code paths, related test functions, phase and remaining work.
The current implementation covers subsets; whole requirements and whole historical
acceptance candidates remain `NOT RUN` until every applicable assertion is exercised.
No historical source CSV has been edited to manufacture current results.

## Deduplication decisions

Use one fixture family for overlapping setup and retain each source outcome. Do not
multiply the number of distinct tests merely because several IDs cite that fixture.

| Shared fixture / outcome | Retained overlapping source IDs | Execution boundary |
| --- | --- | --- |
| Parent matrix, depth, invalid moves | CF-02, ST5B-I1-02, ST5E-T-14; broader TC-07-I/SV-27-I/S4C-019-I | Existing organisation tests cover core parent/depth transactions; broader cases still retain their other assertions |
| Unfiled and stable identity | CF-01, ST5B-I1-04, ST5E-T-15; S4C-003 | Share organisation setup; preserve distinct timestamps, source/history and reference assertions |
| Source survives history expiry | CF-22, SV-10, S4C-004 | Share source/history fixtures; presentation toggles and full source-reference checkpoints are separate retained outcomes |
| Missing/failed model readiness | CF-24, SV-09, SV-26, S4C-024, ST5E-T-10 | Share manager setup; actual installed-model/offline host path is still pending |
| Source revocation | ST5B-I1-08; CF-10/SV-08/SV-25/S4C-025 later-context variants | I1 policy test does not certify M1 catalogue/overlay propagation |
| Protected portable export | ST5E-T-21, ST5B-I1-07, ST5E-T-12 | Shared auth/export fixtures; do not drop recognition access or other-scope assertions |
| Native recovery | ST5B-I1-05, ST5E-T-22, ST5E-T-20 | Subset and whole-library recovery have separate tests; neither certifies Android OS-backup exclusion |
| Shared exercises and membership | CF-25, CF-28, S4C-002, S4C-026 | Exercise fixtures cover shared revisions/status, heading stability, explicit reassignment/undo, independent copies and native v3 attachments and persisted view anchors; general history, caret positions and Android host subscriptions remain open |
| Post-build quality and pen use | CF-29/CF-30, ST5E-T-26/ST5E-T-27 | Separate deferred owner-private checks; synthetic adapters never substitute |

The candidate ledger records all other fixture families and all unabridged expected
outcomes. Mixed-phase aliases remain separate; generic repeated wording such as
“run after implementation” is not grounds to merge different M1 features.

## Search and Help gaps closed in the test mapping

`NB-SUP-001` now points explicitly to
`writing::search_matches_literal_accepted_content_never_raw_or_locked_data`.
It covers literal accepted content, titles, raw-ink exclusion and access checks.
Diagram labels/headings, new entity paths and scale remain unfinished.

Exercise tests additionally cover title non-disclosure from protected headings,
protection through master/history/inference/links/export, and failed shared saves.
V2 migration tests seed actual v2 source observations and history before upgrading;
an empty-schema upgrade alone is not treated as preservation evidence.

`NB-SUP-002` now points explicitly to
`converters::offline_help_covers_recovery_and_unsearchable_ink_without_remote_dependency`.
It covers bundled recovery topics with no remote lookup. Full contextual integration
and usability are unfinished. These replace historical `TBD` test references in the
current map, not in the approved text, and do not declare whole requirements passed.

The inherited `SUP-002` text concerns unrecognised-ink search, although its editorial
arrow names `NB-SUP-002`; the current semantic link is `NB-SUP-001`. `SUP-003`
concerns Help although its old arrow names model management; it now maps to
`NB-SUP-002`. `SV-16` has an M2 label but I1 canvas-embed camera/gesture semantics;
the current plan retains its I1 backend invariant and post-gate host action.

## Required next validation

Complete and test every open group in `backend-gaps.json`, then close applicable
assertions individually with executed evidence. Add bounded malformed-format/fuzz
coverage, remaining interruption boundaries, scheduler/resource contention,
model-install rollback, graph/source lock paths, full backup migration/restore,
and automated Kotlin/Android ABI/security configuration checks. Keep device quality
and H.I. tests separate. None of the 16 proposed numerical SLOs is approved here.

The existing process-exit test ends without Rust destructors; it is stronger than a
normal close/reopen test but is not a physical power-loss test. SQLite-full testing
uses SQLite's actual page limit to produce a real SQLITE_FULL transaction failure;
separate reserve tests allocate and release real files, but do not establish Android low-space orchestration. Cargo `--offline` is dependency
offline mode, not an OS network-egress audit of a future native SDK.

## Current increment boundaries

Capture fixtures exercise classified observed events, cancellation, bounded admission,
failed-save retry, durable idempotency and cooperative scheduler leases. Separate
Android fixtures exercise synthetic MotionEvents under Robolectric. These are not
physical tablet event-delivery tests and do not establish a device latency SLO. Object fixtures exercise
editable data, grouping, camera independence, protection and native subset copies. Image
fixtures are signature-level byte fixtures, not successful image decoder tests.

Engine fixtures use compiled synthetic adapters and local bytes; they exercise hash
verification, consent, probe/transaction failure, rollback, defaults and concurrent
change. They establish no recogniser accuracy or sandbox isolation. PDF fixtures
exercise source/anchor metadata and cache policy. Android provider fixtures also cover
bounded off-thread opening, deadlines, cancellation, revocation, late descriptors and
invalid-render/failed-reply descriptor cleanup. New native host fixtures exercise
bounded private read/revision tokens and source/lock/restart/lineage rules. The Android
pipeline fixtures use the actual desktop JNI library plus synthetic provider and
Messenger transport to exercise frozen source hashes, live permission rechecks,
revocation, expiry, low storage and raw-pixel framing. Robolectric pipes are file-backed
and its dup copies bytes; these are not live-pipe/cross-process/device tests. Trusted source-selection fixtures now exercise actual JNI prepare/accept callbacks,
non-mutating/rejected review, fresh bytes/grant checks, stable-ID regrant from revoked
state, stale generation/locks and the accepted-commit cancellation boundary. A storage
fault fixture rolls back source/anchor changes while retaining independent evidence.
Retained-source fixtures now exercise explicit nonpersistent staged retention,
chunk/total admission bounds, wrong bytes/order, rejection/relock/restart, shared
live/retained lease capacity, old-edition provenance and scoped access despite
independent authority. Actual JNI Android fixtures render managed copies after
provider loss, historical snapshots after live revocation, reject tampered/dropped/
misaddressed/out-of-range chunks before parsing, and retire frames during retention
revocation/final validation. Provider opens are asserted absent on retained reads.
Typed retention fixtures now follow actual JNI picker link/regrant acceptance with
explicit managed/historical acquisition. They exercise default-link/no-copy behavior,
independent authority/export validation, multi-chunk freezing, final provider recheck,
changed bytes, low storage/oversize, cancellation after staging, blocked-provider
backpressure while notes remain writable, relock and temporary/native stage cleanup.
Synthetic native seams cover final-submission receipt delivery past cancellation/
timeout and conservative expiry after delayed replies. A cache-disposal fault
closes acquisition without disabling authored-note operations. These do not substitute for
actual Binder/provider/device permission or complete external lifecycle acceptance.

Backup fixtures exercise password/recovery restoration, Trash/history/membership
undo, rich object/view/anchor/profile/proposal/capture state, authorised source assets,
malformed authenticated data, stale scope and refusal to overwrite an existing file.
Derivative fixtures cover scope/authentication, raw-ink exclusion, vector escaping,
hidden-point rejection and pagination geometry. Paginated selectable-Unicode/vector
maths PDF writing is implemented and tested; complete image/embed/canvas fidelity
remains open.


Ownership fixtures exercise duplicate native hosts and aliases, independent
libraries, failed initialization, actual subprocess locking/abrupt exit and original
ink recovery. A SQLite ownership unit fixture verifies exclusive mode across commits and rejected/
closed duplicate connections without opening extra raw database descriptors. Native close drains through an active
verifier and queued capture despite surviving Arc references, and rejects self-join
from a verifier. Standalone JNI checks actual concurrent close waiting while another
library remains responsive. Actual JNI Android fixtures reject duplicate/root aliases
before factories/cache cleanup, distinguish native drain from provider/parser
retirement, close failed factories/native opens and reopen after full shutdown.
An independent JVM probe verifies that rejected duplicate/canonical alias opens
retain the actual app OS lease until full retirement. Native-open failure retirement
is checked before any explicit close call. These establish desktop cooperative
locking and simulated Android resource lifetime,
not physical-device Binder termination or final APK/process enforcement.


Interrupted capture fixtures check immutable active snapshots, stable begin-order
identities, aggregate bounds, no fabricated final sample, rejection of active
prefixes at all persisted admission boundaries and exact retry identity including
termination. Schema-9 migration preserves raw source payload/digest and receipts;
native v5/import and encrypted schema-10 restoration preserve interrupted provenance,
while downgraded containers reject it. Legacy schema-8/9 and native v1-v4 readers
remain covered. Android accumulator/MotionEvent fixtures distinguish lifecycle
interruption from rejected platform contacts; actual JNI capture/reopen verifies
one observed prefix, its interruption reason and exact measured-zero/raw doubles.
The journaled session integration below adds durable active-prefix recovery; Android
priority/resource completion and physical-device acceptance remain open. No physical
pen interruption or save-latency guarantee is established.


Durable active-journal fixtures cover exact prefix updates, stale/rebound headers,
checksums, quotas/no eviction, cancellation, review-before-publishing, exact replay
after ambiguous native acknowledgement, foreign-library rejection, retained torn
files and independent-session progress. A real JVM child halts after file sync,
rename and directory sync; reopening reconciles valid records and preserves observed
bits as reviewable interrupted evidence. Actual JNI session fixtures distinguish
checkpoint/commit receipts, reject stale contact resurrection, seal on full close,
review/discard with fresh permission, write beside blocked protected recovery, and
prove cancelled observer futures cannot drop admitted journaled/legacy ink. Native
fixtures verify receipt scope/target and private identity without granting access.
These are process/filesystem/desktop-JNI tests with synthetic MotionEvents, not
physical power-loss, pen-latency or Android device acceptance.

## Android reserved writer admission

`BackendWriterTest` deterministically gates running actions to exercise separate
count and byte budgets (including running input), authoring FIFO over queued reads,
capacity recovery, cancelled observers, draining shutdown, sticky authoring failure
and coalesced status. No status callback or per-subscriber event queue runs on the
writer. `BackendAdmissionIntegrationTest` uses actual desktop JNI and synthetic
MotionEvents: a full optional-read lane still admits checkpoints/completion, queued
reads observe the subsequent commit, large input exhausts bytes before count, and
capture/relock ordering checks fresh authority while retaining denied work for replay.
These are backend/Robolectric fixtures; device latency, load and process acceptance
remain unproven. Counter snapshots never substitute for per-ticket commit receipts.

## Revision-bound selection restoration

`positions` covers Unicode scalar offsets and direction, reopen, explicit stale
revision handling, future/out-of-range rejection, rollback, maths child/leaf and
component targets, multi-block ranges, cross-owner rejection, exercise reassignment,
fresh block protection, exact original ink, global-ID remapping, stale copy/backup,
legacy view migration and downgraded container rejection. Host and actual desktop-
JNI/Robolectric fixtures exercise SaveView/RestoreView before/after an explicit edit.
A preliminary region fixture correctly received ReviewRequired for an attempted
whole-block replacement; it now uses the explicit region-edit API. No bypass was
introduced. No tablet caret/gesture, automatic rebase or production H.I. is claimed.

## Workspace restoration and privacy

`workspace` covers reopen with independent master/exercise views, tools, camera,
weights, source samples and PDF position; fresh protection/redaction; Keep without
private-target disclosure; app lock; invalid/stale layout, rollback and clear; Trash/
restore and atomic purge reference retirement; encrypted restore with revoked PDF
readiness; malformed order/downgraded tables; default legacy migration and native
copy without workspace activation. Host and actual desktop-JNI/Robolectric fixtures
save layout, fully close/reopen and discover the previous view/tool, retaining current
access boundaries. Existing legacy fixtures use their genuine pre-workspace table
sets, so older bookmark/interruption downgrade tests still check those feature tags.
No physical tablet pane layout, provider grant persistence or H.I. behavior is claimed.

## Stroke appearance provenance

`stroke_style` covers begin-bound configuration, immutable active headers, invalid
styles, exact receipts, reopen/correction/native copy, schema-12 migration without
payload/hash/receipt rewriting, encrypted style restore/downgrade rejection and SVG
RGBA/width/dots/fallback warnings. Android tests cover begin-bound style/legacy wire,
STEMAC02 exact style bits and changed-header rejection, a frozen independent STEMAC01
record without invented style, and actual JNI capture/retry after mid-contact style
change. No pressure/nib inference, implicit physical conversion or production H.I.
is established by these fixtures.

## Canvas layers and embed visibility

`layers` covers implicit defaults, independent explicit/default/empty embed overrides,
missing references and identity recovery on undo, monotonic revisions, rollback,
reopen, exact source retention, malformed members/aliases, fresh protection,
layer/z ordering, nested protected/broken placeholders, output-byte paging with
explicit continuation/revision metadata, native remap, default-visibility derivatives,
encrypted state/undo restoration and current/historical downgrade rejection. Legacy
schema-13 migration compares exact original payload/hash bytes. The actual desktop-
JNI/Robolectric workflow sets defaults/overrides, reads both projections and undoes
an override without changing source defaults. The initial backup fixture exposed the
canvas-keyed payload reader case; the validator now checks that explicit key instead
of assuming an id column. No tests bypass source protection or enable production H.I.

Saved-view discovery fixtures cover restart without workspace references, visible-only
pagination, fresh block/app locks, master/exercise separation and stored identity
corruption. The actual JNI workspace-reopen fixture discovers its saved view before
restoring the pane. This does not close complete authoring configuration acceptance.

Retired capture recovery fixtures exercise actual core subtree Trash/purge/reopen,
app/ancestor/target authority, protected source preservation on denied/faulted purge,
and absence of ordinary read/capture access to tombstones. Android JNI tests use
explicit synthetic persisted retired-target states (not the still-unexposed deletion
host workflow), verify review authentication/relock/discard and reject replay; journal
fixtures include sealed blocked records and exclude active contacts.

Deletion host fixtures cover explicit confirmation, immutable action kind, library
generation checks, expired/bounded/released/host-local reviews, relock/authentication
retirement, hidden dependency redaction, revision-bound restore and source retention
across reopen. Core fixtures cover mixed trashed/purged descendants, commit rollback
and current Trash permissions/pagination. Android actual-JNI tests now execute real
Trash/restore/permanent deletion, preserve negative-zero source samples through
restore and retire source on purge; retired-target recovery setup also uses the real
host commands rather than synthetic persisted flags.

Layer-target capture fixtures check atomic source/membership undo/redo/reopen, exact
receipt retry after layer removal, rejection of new missing/wrong-kind/protected
destinations and rollback. Android fixtures freeze STEMAC01/02 golden records,
preserve destination through recovery, reject destination rebinding and exercise
real JNI fixed-layer capture/retry/missing-layer restart/review/discard. Legacy
request tests reject unpaired UTF-16 without changing valid vector encoding.

Capture receipt privacy fixtures were first run against the prior implementation
and reproduced block/exercise mismatch-before-authority failures. Regression cases
cover equal/changed valid payloads and targets under current block, exercise and
actual-owner locks, authorised exact/conflicting retries, unchanged original ink,
protected malformed block content without unauthorised deserialisation, and a
synthetic SQL-owner/payload-owner splice in ordinary block reads (reproduced before
the common read guard was applied). An
actual Android/JNI fixture compares the complete denied JSON responses for equal
and changed retries, with null data/generation and no raw-content diagnostics.

Combined deletion fixtures check native non-consuming review validation and current
proof/generation, capture-target privacy (including previously purged descendants),
actual JNI subtree Trash and purge, exact keep/discard behavior across active journal
and legacy outbox, active-prefix invalidation, unrelated/incomplete/unknown retention,
relock rejection and post-commit cleanup failure reporting. Legacy pending records in
the mixed-store fixture are injected while the app writer is idle to simulate records
awaiting acknowledgement; no production untrusted file writer is introduced.

Authoring configuration fixtures cover preset selection/CAS/rollback, explicit source
calibration, camera/selection/raw-ink invariance, stale bookmark preservation without
consuming document Undo, fresh protection, all guide kinds/XML escaping/export omission
and density rejection, view-local native copy, schema-15 backup, older-tag rejection
and byte-preserving schema-14 upgrade. Actual JNI tests apply presets during an active
contact, verify only future-contact styles change, preserve measured coordinates, query
export omission and restore the selected preset/camera after full backend reopen.

## Android recovered-copy workflow coverage

`NotebookRestoresTest` adds actual-JNI Robolectric coverage of encrypted input,
private review, fresh publication and independent recovered-root opening. Its
blocked provider/read/cleanup fixtures check native versus full-close ownership;
cancellation is acknowledged only after actual operation retirement. Catalogue
markers and current source grants are checked separately from restored content
protection. Malformed input, credential and quota failures must not change the
live library or pending capture files. Post-publication ambiguity keeps the copy
and receipt. Real provider/ART/device durability, hard disk quota, bounded native
semantic validation and explicit orphan review/cleanup remain required work.

## Unfinished restore retirement

Restore-stage coverage separates purpose/owner marker recognition, bounded metadata
inspection and file effects. Host fixtures test same-size changes with restored
modification times, unknown marker extensions/versions, namespace replacement,
links/nested or incomplete listings, partial transfer/deletion and protocol-only
cleanup. Actual-JNI lifecycle fixtures test fresh confirmation/proof, foreign/stale
capabilities, root ownership through blocked work, preservation of original drafts,
rejected duplicate SQLite opens after hardlink inspection, restart continuation and
an actual child-JVM abrupt exit. Quarantined unknown bytes are never treated as
validated notebooks or silently deleted. Static recovery Help is checked before
library opening and while app content is locked. Real ART/provider/device/storage
acceptance, hard physical quotas and whole-graph validation bounds remain separate.

## SQLite-backed whole-library validation

`backup::validation::tests` retains the old aggregate validator as a test-only
oracle for a rich graph and independent semantic corruptions. Cases include SQL
identity/owner mismatches, containment/depth/cycles, ordered source membership,
hashes, current/history regions and identity aliases, exercise versions, links,
image MIME/signature/hash, groups, layouts, layers, embed cycles/cameras and
view/bookmark constraints. A separate case explicitly demonstrates stronger
rejection of orphan raw stroke rows that the old reconstruction omitted.

An actual encrypted streaming restore exceeds the former subset limits with
10,001 entities, 100,001 blocks and 10,001 exercises and verifies counts, literal
incorrect working and stage retirement. A private SQLite fixture validates three
24 MiB image assets without collecting their bytes in Rust, then rejects digest
corruption. A 2,048-edge embed chain validates without native recursion and fails
a closing back edge. Deterministic cooperative cancellation during semantic work
must remove all private work tables/indexes, as must successful and failed checks.
These are synthetic host correctness cases, not measured device SLOs or a hard
native memory/disk quota. Individual-record allocations, SQLite calls, streaming
cell allocation and complete scratch/publication budgeting remain to be covered.

The pinned SQLite query-plan test requires covering primary-key traversal without
a temporary sorting B-tree and checks negative/zero edge cursor boundaries.
A legacy encrypted restore test injects cooperative cancellation during semantic
validation and requires an empty staging directory and unchanged source. Isolated
follow-up execution is recorded as development evidence; final Git-snapshot
validation is recorded separately.

Source-grant retirement coverage checks a multi-row source table, complete
relationship-field preservation, revoked-versus-missing availability, fresh
recovered-library identity and rollback of an already-modified prefix on
cancellation. Production retirement uses indexed rowid steps without a table-wide
Vec or an open source cursor while updating that same table.

A real WAL/second-reader checkpoint fixture requires rejection while a reader
prevents truncation, successful checkpoint after release and intact subsequent
SQLite authoring. A non-WAL/no-op checkpoint cannot acknowledge stage sync. The
open stage uses SQLite's FULL synchronous transaction/checkpoint path without
opening/closing a raw stage descriptor that could disturb POSIX SQLite locks.

## Backup publication history

`BackupCatalogueTest` exercises restart/snapshot matching, independent libraries,
provider readback versus sync, inclusion/omission retention, transaction rollback
and lost acknowledgements, bounded descending pagination, unknown/future database
preservation, hard/symbolic links, namespace replacement, trusted ancestor aliases,
missing-index refusal and invalid/corrupt record fields. Platform SQLite is used
only for the metadata fixture, never for a Rust-owned notebook database.

`BackupCatalogueIntegrationTest` uses actual JNI with local encrypted publication,
full backend restart, newer edits, unclassified pending capture evidence, fresh
app lock/relock privacy, authoring failure, separately acknowledged catalogue
failure and close during an entered metadata commit. Full close must retain the
app lease until that worker really retires. Existing local/provider backup tests
also run. These checks are host/Robolectric components; destination availability,
physical/device/provider durability and complete backend acceptance are not
inferred. Offline Help distinguishes saved state, past publication records,
exclusions and actual external recovery files.

## Scheduled-backup foundations (not a completed scheduler)

`BackupSecretProtectorTest` uses actual JCA AES-GCM with a test key provider to
check ciphertext/context tampering, exact managed-buffer clearing, bounded codec
and recovery-key domains, alias non-overwrite, uncertain store callbacks, typed
cancellation/interruption, blocked consumer/store/deletion lifetime, deletion
retry and completion outside the registry monitor. It does not execute Android
Keystore or establish hardware backing.

`BackupPersistentAccessTest` uses synthetic provider/permission adapters to check
explicit consent/flags, before-grant decision recording, prior shared rights,
owner/epoch/single-use review, invalidation during inspection/proof/recording,
changed authority/directory/permissions, post-grant cancellation/failure and lost
acknowledgements. It never creates a backup or grants source-content access.

Both production Android adapter sources compile and lint. Real Keystore/provider
persistence, a durable consent/credential store and the scheduler service/run
lifecycle are not proven by these component tests and remain implementation work.

### Device-local credential/consent store component coverage

`BackupPolicyStoreTest` uses Robolectric SQLite and injected JCA AES-GCM keys to
exercise durable intent-before-key creation, restart decryption, required consent,
rollback/lost acknowledgements, consumer-held retirement, failed deletion and
restart retry, purpose/library/backup binding, bounded discovery and malformed
blobs, permission decision/outcome matching, cancellation and foreign/linked
namespace preservation. See [the store contract](../formats/BACKUP_POLICY_STORE.md).
No scheduler, real Android Keystore/provider, device or physical-interruption
acceptance follows from these host component checks. Whole requirement statuses
remain incomplete.

`BackupSchedulePolicyTest` covers persisted Arming/Enabled/Disabled transitions,
exact registration revisions, stale/duplicate/concurrent job admission, active
run retention during disablement and credential retirement, rollback, lost
registration/run acknowledgements, restart interruption, exact recorded provider
consent, immutable outcomes and replacement with fresh reviewed authority. It is
a SQLite state-machine test; no real OS job is registered by these fixtures.

### Shared runtime ownership

`SharedNotebookPoolTest` exercises independent pending/claimed leases, cancellation
without shared-open cancellation, actual-release gating, failed close/open,
external close/replacement, bounded root/reservation admission, interrupted
observation and slow openings without retirement starvation. `NotebookRuntimeTest`
uses actual desktop JNI through Robolectric for overlapping consumers, last-owner
reopen, canonical aliases, existing-only missing-library rejection, preserving
direct exclusive ownership and main-thread refusal. Run these with the existing
six `BackendOwnershipTest` cases and lint. No real OS job/device acceptance is
implied; see [the runtime contract](../formats/SHARED_NOTEBOOK_RUNTIME.md).

### Opt-in scheduled backup workflow

`NotebookSchedulesTest` uses actual native encryption/publication and JCA test keys
to exercise initial key offer/consent, due backups, separate local/provider
acknowledgement, recovery-key reveal/delete, unused intent cleanup, source and
directory invalidation, quotas, multi-copy publication and preserved protected
verification. `NotebookBackupJobServiceTest` exercises worker-only existing
opening, malformed extras, token-based stop across parameter instances and actual
retirement before releasing the service slot. `BackupJobSchedulerTest` verifies
platform periods, opaque extras, owned-job checks, rejection, cancellation and
unroutable-root refusal. `BackupPolicyMigrationTest` checks exact known v1 upgrade,
retained credentials/run fences, future-file preservation and rollback.

Known SQLite-file mutation fixtures explicitly use NO_LOCALIZED_COLLATORS; the
policy tamper test also counts actual decrypt entry so an earlier schema rejection
cannot falsely stand for AEAD-tamper coverage. Include existing manual local/
provider, catalogue/JNI, pool/runtime and ownership regressions with lint. These
are host correctness checks, not actual Keystore/Binder/provider/device/timing/
physical interruption acceptance or completed protected/independent-root
scheduling. See [the workflow contract](../formats/SCHEDULED_BACKUPS.md).

## Structured table-cell ink

`table_cell_ink` covers explicit ticket/revision/cell-bound capture and exact retry,
selected ordered digital-ink recognition, cell-only acceptance, stale neighbouring
proposals, manual corrections, cross-batch session order, unknown/colliding targets,
fresh protection and injected rollback. Undo/Redo retains appended observations;
independent native copy remaps cell/region/stroke identities together. Both encrypted
backup formats preserve cell targets, and current/history/Undo-only bindings cannot
hide beneath an old format tag. Derivative text occurs once per accepted cell,
with an explicit warning for omitted unconverted cell handwriting. A native Host
fixture uses CaptureCell and the scoped proposal path; it is not Android capture
journal, physical grid hit-testing, recognition quality or whole-requirement proof.
Existing authoring, region, migration, pressure, object, engine-history, recovery,
workspace and editing suites exercise the schema19/native11 compatibility changes.
