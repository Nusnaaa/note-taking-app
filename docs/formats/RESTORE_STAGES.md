# Private unfinished-restore files, version 1

This is an app-private recovery protocol, not a portable notebook format or a way
to activate unvalidated data. Files remain under the selected no-backup library
root. Normal notebook backups do not include these unfinished copies. No provider
grant, credential or model resource is recorded here. Private SQLite scratch is
not independently encrypted at rest; no device-confidentiality claim is made.

## Preparation marker

A fresh mode-0700 `notebook/restore-staging/<stage UUID>` directory precedes native
decoding. Its mode-0600 `stage.json` is created without overwrite and synced along
with the directory and parent. Its exact fields are:

```
{"format":1,"purpose":"restore-stage","library":"<working-library UUID>","stage":"<directory UUID>","created_at_millis":0}
```

The timestamp is the observed creation time, not a performance measurement. The
library identifies the host that owns the private files, not necessarily the
library carried by the imported backup. Marker reads are bounded at 16,384 bytes;
unknown fields, versions or purposes cannot authorise permanent discard.

Identified scratch may contain that marker and files of one native UUID family:
`.stem-stream-restore-<UUID>.db`, with optional `-wal` and `-shm`. Direct children
must be regular, single-link, private files owned by the directory's UID. A name
alone never identifies a file as disposable. Missing/malformed/foreign markers,
extra files, nested directories, links and incomplete listings are unclassified.
An exactly empty private recovery directory may be identified as empty without a
marker; it contains no file data to discard. Normal retirement removes only its
exact unchanged marker after native scratch/descriptors retire, then the empty
directory. Other bytes remain for deliberate review.

## Review capabilities

NotebookRestores owns one native restore reservation during a bounded page of
inspection/review. Pending decode or other stage reviews cannot share it. App
LibraryOwnership spans provider, native, inspection and cleanup work. No caller
provides a filesystem path through notebook/model JSON. Public items/cursors are
opaque, owner/epoch-bound objects with generic labels and bounded metadata.

Snapshots record device/inode, link count, UID/mode, byte length, modification and
change times, and a digest of the bounded JSON marker. Payloads are **statted, not
opened or hashed as raw database files**: closing such an FD could disturb this
process's SQLite locks. This is metadata freshness under cooperative app/native
ownership, not a cryptographic content-integrity or hostile-kernel guarantee.
Root directory identities are anchored; replacement cannot redirect inspection.
The host Unix attribute view is used with an Android lstat fallback. Host tests do
not establish real-device filesystem/fallback behaviour.

Page and direct-child retention limits bound metadata memory. Enumeration checks a
cooperative deadline; blocking OS calls are not claimed hard-terminated. Incomplete
or nested listings have no complete byte total and cannot grant discard. Logical
file-byte observations/removal counts are not physical disk-allocation guarantees.

## Retirement and quarantine

A fresh current app-access check, explicit confirmation and fresh platform proof
precede mutation. Snapshots are revalidated after proof and immediately before
transfer. An opaque review is consumed by one selected action; changed files need
fresh review. Unknown data can only be quarantined, not permanently discarded.

The worker reserves a fresh mode-0700 `notebook/restore-retired/<retirement UUID>`
container and syncs a mode-0600 `retirement.json` with these exact fields:

```
{"format":1,"purpose":"restore-retirement","library":"<working-library UUID>","id":"<retirement UUID>","original":"<original entry name>","disposition":"Quarantine","created_at_millis":0}
```

`disposition` is `Quarantine` or `DiscardRecognised`. The original name is private
protocol metadata, never a public item label. The selected entry is atomically
renamed into the freshly reserved container as `entry` on the same filesystem;
source/target directory changes are synced. Private-root ownership excludes other
cooperating writers; this does not claim a hostile-filesystem no-replace primitive.

After transfer, identity/metadata are checked again. Quarantine preserves every
byte/link without traversing or interpreting unknown payloads and does not free
their storage. Known discard checks each file again, unlinks only the reviewed
scratch/marker, and removes empty directories. It does not securely erase storage.
The working database and private capture journals are outside this namespace.

Once transfer is entered, cancellation cannot hide possible effects. Errors,
lifecycle cancellation or uncertain sync report `Uncertain`, retaining whatever
remains for restart discovery. Removal-byte counts on an uncertain receipt describe
acknowledged completed deletions, not a complete final inventory. Close retains
ownership until the worker and cancellation work actually retire.

`reviewStages(retired=true)` discovers retained containers. Identified interrupted
scratch can be re-reviewed and discarded; an identified protocol-only record or
empty reservation can be cleaned separately. Unclassified quarantined bytes stay
preserved and cannot be repeatedly treated as known scratch. Ordinary JSON never
becomes a path, source-content or deletion capability. Quarantine is not automatic
activation or a new independent backup.

Hard staging allocation quotas, individual-record/SQLite validation allocation
controls, complete orphan salvage/export and real-provider/device crash/storage acceptance remain
open. Do not promote this protocol's component evidence to backend completion.
