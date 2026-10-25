# Scheduled-backup foundations (scheduling is not enabled)

These components now support the [reviewed scheduling workflow](SCHEDULED_BACKUPS.md)
through a public controller and JobService for the primary app root. Constructing
or opening a backend still enables no job, grant or stored credential. Protected
backup delegation and independent-root job routing remain required unfinished
work. Manual backup/history stays independent.

## Device credential wrapping

`BackupSecretProtector` accepts purpose-bound contexts: library UUID, fresh
credential UUID and `SchedulePassword` or `RecoveryKey`. These references do not
grant notebook access. The storage/controller layer must first record explicit
credential-storage consent and validate the password through native backup
creation; this primitive does not duplicate password-quality policy.

The version-1 encrypted envelope is `STEMKEY1`, version byte 1, a 12-byte nonce and
AES-256-GCM ciphertext with a 16-byte tag. The associated data binds the format,
purpose and both UUIDs. The encoded secret uses exact UTF-16BE code units, with
1–1024 input units (39–2085 envelope bytes). Recovery keys additionally require
exactly 64 hexadecimal characters. These are defensive codec limits, not entropy
or device-performance claims. No plaintext secret is converted into a production
Java String. Temporary managed byte/character arrays are cleared on success,
consumer failure and cancellation; callers own their original input/copies.

The Android adapter uses distinct Android Keystore key aliases without exporting their keys, random
GCM nonces and unlocked-device-required keys. It requires a secure unlocked device
and provides no plaintext/software-key fallback. This does not claim hardware
backing. Android documents limitations for unlocked-device-required keys on older
releases; failure must be reported, preserving independent portable credentials.
See [the Android key policy reference](https://developer.android.com/reference/android/security/keystore/KeyGenParameterSpec.Builder#setUnlockedDeviceRequired(boolean)).

Key leases include encryption, the encrypted-store callback, decryption and the
actual consumer callback. Retirement denies new opens and completes only after
in-use callbacks, buffer clearing and key deletion retire. A failed deletion can
be explicitly retried. Provider calls and future continuations run outside the
registry monitor. Callers must not synchronously await retirement from their own
in-use callback. The persistent store must mark a reference retired before
requesting deletion and preserve its encrypted row on uncertain failure.

A commit callback can commit before losing its acknowledgement. The primitive
therefore keeps the key on failure rather than guessing that deletion is safe.
Durable intents and credential retirement now have an internal
[store/vault](BACKUP_POLICY_STORE.md); orphan review still needs controller integration. Atomic referencing-policy
disablement now precedes credential retirement. Keystore/device behavior is not
inferred from the JCA test-key
provider used in the host correctness tests.

## Persistable provider permission

`BackupPersistentAccess` is a separate reviewed operation. It does not enable a
job, store a password or create/open a backup document. A new permission needs
read/write/persistable picker flags, current app authority, unchanged directory
identity/label and a fresh trusted verification callback, plus explicit persistent
access confirmation. Existing complete access may be reviewed without retaking it.

Review capabilities are owner/epoch-bound and single use. Invalidation during
inspection, verification or consent recording prevents entry to the OS grant
phase. The caller's mandatory decision recorder must durably store the consent
before `takePersistableUriPermission` is called. Once grant entry occurs, failure,
interruption, invalidation or a lost outcome acknowledgement returns `Uncertain`
with observed rights where available. It does not pretend that nothing happened.
No rollback automatically revokes permission that another feature may use.

Persisted access is not a permanent availability guarantee; moved/deleted targets
can require renewed permission. See [Android's storage access guidance](https://developer.android.com/training/data-storage/shared/documents-files#persist-permissions).

## Remaining integration

The public controller, native initial backup/key offer, device credential and
provider-consent integration, reviewed enable/disable, due-run outcomes,
recovery-key reveal/delete and primary-root JobService are now implemented.
Still required: narrowly authorized protected-content scheduling and routing for
independent recovered roots, plus real device/provider/Keystore and interruption
acceptance. Background verification is never an always-true substitute for
owner authentication. No OS notebook cloud backup, mandatory account, remote
inference, model installation or production H.I. is introduced.
