# Device-local backup policy and credential records

The internal `BackupPolicyStore` and `BackupCredentialVault` persist credential
and provider-consent authority. They do not register or enable scheduled work.
Persisted [schedule configuration/run state](BACKUP_SCHEDULE_POLICY.md) now exists
in the same store; a [shared runtime](SHARED_NOTEBOOK_RUNTIME.md) is implemented, while its scheduling
controller and JobScheduler service integration remain pending. Manual backup publication history remains separate.

## SQLite ownership

A dedicated private directory holds `policy.db`, purpose
`STEM-BACKUP-POLICY-1`, user version 1. Only the exact metadata, credentials, permissions, configurations and runs
table definitions are accepted; unknown/future schemas, additional
objects and unexpected files are preserved and rejected. Existing files are
validated read-only before a write connection is admitted. Private anchored
namespace, leaf mode/UID/link checks and no-follow inspection precede SQLite
opening. SQLite owns all database descriptors. Transactions use FULL synchronous;
only directory descriptors are synced after closing SQLite connections.

This database is device-local metadata, never a portable notebook/backup and
never publication history. Its future controller must place it under
Context.noBackupFilesDir, hold app/library ownership and run operations on the
optional worker. No instance is constructed by production NotebookBackend yet.

## Credential state

Each credential has a fresh UUID, library, purpose, explicit device-storage and
independent-recovery acknowledgement timestamp, and optional specific backup UUID
(required only for RecoveryKey). The store requires both acknowledgements before
it creates Intent. The caller must separately obtain trusted fresh verification
and validate scheduling passwords through native initial-backup creation. These
internal booleans are not a public verification mechanism.

Intent is durably committed before creating a Keystore alias. Only a complete
bounded encrypted envelope changes Intent to Ready. No passphrase/key plaintext
is written to SQLite, converted to a production String or put into publication
history. The existing AEAD codec binds library, purpose and credential UUID.
SQLite checks envelope length before extracting a blob into Kotlin; format bounds
are checked before use, and AEAD authenticity is checked before the consumer.

Retired denies new consumer callbacks. Its encrypted envelope is retained until
actual key consumers retire and Keystore deletion acknowledges. Deleted keeps a
reference tombstone and drops ciphertext. Reference reuse is rejected. A failed
or lost deletion acknowledgement is retryable after restart. No uncertain commit
triggers guessed rollback or automatic key destruction. Incomplete Intent,
Retired and Deleted rows are discoverable through bounded library-scoped keyset
pages, without enabling a schedule. The vault rechecks Ready inside its key lease
before exposing a plaintext array; the lease spans actual consumer retirement and
array clearing. Caller-owned input arrays remain the caller's responsibility.

A committed Ready row with a missing/unusable device key remains unavailable;
there is no plaintext/software-key fallback or claim of recoverability through
the originating device. Independent portable credentials are still necessary.
Credential retirement now disables and changes the revision of referencing
schedule configurations in the same transaction. Running-work fences remain
until actual retirement. No OS scheduler currently references these rows.

## Persistable provider consent

Record the reviewed decision before invoking the OS permission request. It
contains a fresh decision UUID, library, exact content-tree identity, prior
read/write flags and explicit consent time. Store a separate, single-assignment
Persisted, AlreadyHeld or Uncertain outcome with observed flags when known.
Known success requires both observed rights; a missing outcome remains unknown,
never success. The original reviewed decision must match when recording an
outcome. Losing an acknowledgement preserves committed state for restart review.
Decisions and outcomes have bounded library-scoped pagination. No destination is
contacted by this store, and no shared UID grant is automatically revoked.

## Verification scope

Host tests use actual SQLite through Robolectric and JCA AES-GCM with an injected
key provider. They cover durable intent-before-key ordering, restart decryption,
consent rejection, lost acknowledgements before/after key creation, rollback,
consumer-held retirement, failed deletion and restart retry, cross-context
rejection, malformed/tampered/oversized blobs, bounded discovery, grant decision
identity and unknown effects, and preserving foreign/linked/replaced namespaces.
These are component correctness tests. Actual Android Keystore, real provider,
physical interruption and JobScheduler behavior are not established by them.
