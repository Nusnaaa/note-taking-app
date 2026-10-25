# Optional scheduled backups — backend workflow in progress

`NotebookBackend.schedules()` owns `NotebookSchedules`. Constructing or opening a
backend creates no job, credential, persisted URI permission or enabled policy.
A private SQLite route map in the application's primary no-backup directory
supports primary and independent library roots. It stores canonical root paths,
filesystem device/inode/UID, library identity and schedule/revision references;
none enter OS extras. Construction and absent-map reads create no routing state.
Only explicit registration persists a route before the OS call. The reserved OS
identifier namespace has 32 slots as a defensive admission bound; a full namespace
returns unavailable and never evicts a library or overwrites a foreign job.
RecoveredLibrary.acquire provides explicit existing-only foreground leases shared
with background jobs; legacy open remains exclusive. Known canonical aliases share a slot; another path with the same logical library
identity cannot duplicate or displace it. Independent recovered libraries retain
their separately minted native identities and require their own scheduling consent.

Unknown/future route schemas, linked files, replaced private registry namespaces,
changed root identity and stale revisions fail closed without repair. Old primary
jobs without a route keep their existing primary-only path; an unmapped independent
job never falls back to the primary or creates a notebook. Device-backed
Keystore/provider/OS/physical interruption and complete I0/I1 backend acceptance
remain separate from host-side routing/delegation evidence.

## Review and enable

All blocking observations belong on a worker. `prepare` takes an explicitly chosen
local directory or provider tree, requested interval, maximum encrypted-backup
bytes and inclusion choices. The interval must meet the adapter's platform minimum;
there is no fixed timing or performance promise. Native initial-backup creation
validates the password and builds an encrypted private artifact. Local directory
identity or provider tree identity is retained with the review. Authoring/access
changes, close, explicit discard and the bounded review expiry invalidate it and
clear the controller's password copy. Callers own and clear their input array.

By default initial review requires current ordinary scope authority. A caller may
explicitly set `allowProtectedContent=true` for initial preparation: the native
worker then requires a fresh trusted owner proof for that exact library plan and
creates only the private encrypted artifact. This permits complete backup setup
without installing ordinary per-note/block/exercise access or depending on the
ordinary authentication scope-list bound. App authority is still required for
owner configuration. This fresh preparation consent/proof is distinct from the
later persistent protected-content delegation consent; no publication, credential
storage or job registration occurs until enablement. A declined proof creates none.

The caller must deliberately copy the initial independent recovery key before
`enable` can accept the review. That copy is caller-owned and must be cleared;
physical separate/offline storage is the user's acknowledged choice, not a claim
that the backend can verify it. Enable requires fresh trusted verification and
separate confirmations for scheduling, device password/per-backup recovery-key
storage, provider persistence where relevant, and the independent-recovery warning.
If the reviewed descriptor includes protected scopes, separate explicit protected
content delegation consent is required before any publication or credential storage.
Losing every portable credential can make an encrypted backup unrecoverable; a
stored device key is not an independently stored offline copy or a reset service.

Provider persistence uses the reviewed consent helper and durable decision/outcome
records before/after the OS call. Possible grant effects return an Uncertain
receipt even if no backup was published. Shared UID permissions are not revoked
as guessed rollback. The initial encrypted backup is deliberately published and
acknowledged separately from credential/policy/job registration. A later failure
retains that known publication outcome and Needs attention, never implying the
file or a possible provider grant did not happen.

Credentials are purpose/library/UUID-bound encrypted envelopes in the separate
private policy store. No password is exposed through public reveal operations, converted into a
production Java String, or put in OS extras, publication catalogue or portable notebook. Policy
starts Arming and becomes Enabled only after the exact OS registration accepts
its UUID/revision. Rejected or uncertain registration remains discoverable;
explicit owner verification/reconciliation is required to accept an observed
matching registration. Startup does not guess that unknown Arming state is enabled.

## Due work and stop

The platform adapter reserves bounded backend job IDs and refuses a foreign or
malformed occupant; independent allocation skips occupied foreign slots. Extras contain only a version and opaque schedule/revision
UUIDs. OS persistence uses RECEIVE_BOOT_COMPLETED, with the service protected by
BIND_JOB_SERVICE. This permission/service declaration creates no job by itself.
OS notebook cloud backup and transfer remain disabled, and INTERNET remains
removed. No account or two-way notebook synchronization is introduced.

The service resolves the exact private job/token route on its worker, checks the
recorded root identity and opens through the shared runtime in existing-only mode,
never creating an empty notebook. Routed root observations recheck that identity;
run admission also compares the expected native library identity. Distinct jobs
queue on one bounded worker instead of excluding each other. Stop matches both
job ID and token, so a different queued job cannot stop the active library. A
stopped queued job retires without resolving/opening its root. It verifies current private policy identity/revision/state,
credential and recorded permission, checks live permission/target identity, and
uses the existing backup/capture/recovery/source guards. Maximum bytes apply to
native encrypted staging on every run. A replaced local directory is not silently
accepted at the same pathname. Provider success remains read-back verification,
not future availability or cloud durability.

Each new schedule seals an encrypted backup-only approval under its own device
key, distinct from the password and per-backup recovery keys. Its payload binds
library, schedule/revision, credential UUID, exact protected-scope/options digest
and the reviewed policy destination, identity, byte cap and interval. The native
scope descriptor uses canonical library UUIDs and includes the app lock and all
entity locks; changed scopes/options require renewed review. Content revisions
within that same scope can be backed up without renewing ordinary read access.
The descriptor alone is metadata and cannot construct the private native permit.

The due worker authenticates that sealed payload and current enabled policy in a
key lease on the optional worker. Verification and owned verifier destruction stay
off the authoritative writer and registry locks. Native permits only feed encrypted
backup creation; they never install ordinary Host access. App/note navigation,
search and history remain locked. Every pull still checks generation, cancellation,
proof expiry and capture/work authority. Android compares an already authorized
artifact stamp through a dedicated boolean native check rather than asking for
ordinary app authority or exposing the current generation. No always-approving
background proof or silent omission is used. Legacy v1/v2 schedules receive no
invented delegation; their protected work remains unavailable until fresh review.

The private policy schema is v3. Exact v2 migration adds only the delegation
reference table; exact v1 migration also preserves credentials/consent/runs and
disables configurations lacking a reviewed limit/target identity. Unknown/future
schemas are preserved and unavailable. Interrupted/failed work remains distinct
from a known publication; generic native errors are not guessed into precise causes.

A run owns a durable active-run fence until actual native/provider/key work
retires. Cancellation observation is not retirement. File-worker, deadline,
cancellation-worker and final file-worker fences drain possible queued cleanup.
The JobService matches stop by job ID and validated token rather than parameter
object identity, retains each admitted slot/runtime reservation or lease through actual
retirement, and does not call jobFinished after stop. Destruction cancels without
interrupting or abandoning an admitted callback. Separate runtime ownership
witnesses cover canceled openings, constructor cleanup and failed close.

## Disable and recovery keys

Fresh verified disablement persists Disabled with a new revision before OS
cancellation, abandons pending preparation/active work and retires the referenced
password and delegation keys after real consumers finish. The receipt separately acknowledges
job cancellation, key deletion and run retirement. It never revokes a shared URI
grant. An interrupted run can be explicitly reconciled only on the controller's
serialized work path with no surviving scheduled producer and drained backup
work; Interrupted never asserts that an external publication had no effect.

Each scheduled backup retains an encrypted recovery-key reference for its specific
application publication UUID. Fresh verified reveal returns a caller-owned array;
delete marks it retired before actual key deletion and keeps uncertain state for
review. Owner-authorized unused-password/delegation retirement handles interrupted setup
without exposing the password; a referenced live policy must be disabled first.
Pagination exposes a scan cursor even when a page contains only non-recovery
metadata, so filtering cannot hide later recovery keys. Retaining a key on the
originating device does not claim independent offline storage. Original passwords,
old external files and caller-owned copies are not silently erased.

A prepared artifact reserves its first publication UUID for key association.
Additional deliberately published copies receive distinct catalogue record UUIDs,
so publishing the same encrypted bytes twice does not collide with receipt IDs.
Known local/provider outcomes and catalogue acknowledgement remain separate.

## Verification scope

Host component tests use actual desktop JNI encryption/publication, Robolectric
SQLite and JobScheduler/service callbacks, injected JCA AEAD keys and synthetic
providers/permissions. They exercise review/enable/run/disable, uncertainty,
recovery reveal/delete, source invalidation, size and target guards, exact known
schema migration, OS token ownership and actual runtime retirement. This is not
Android Keystore, Binder/ART, real provider, device timing, physical power-loss or
whole-backend acceptance. The eight backend gap categories remain open. No
production Android H.I. is introduced by this workflow.
