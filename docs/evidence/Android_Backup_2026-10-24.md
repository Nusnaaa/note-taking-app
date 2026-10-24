# Android backup controller — 24 October 2026

Validated source: `7964c2fc4e4a783140ca55d01778fd004d8d4291`, branch
`phase-a/backend`. Backend acceptance remains **NOT MET** and production H.I.
remains gated. This implements trusted Android local-file backup orchestration;
it does not close the full portability, recovery or Android/device contracts.

## Implemented workflow

NotebookBackend owns NotebookBackups, with one retained review/artifact, one
optional operation and a bounded worker queue with cleanup capacity. Review,
encryption, payload file I/O, sync/publication and cleanup run on its worker.
Private-directory metadata barriers and the native generation query remain
serialized on BackendWriter; they are bounded work checks, not a device-latency
claim. Private folders use first-entry directory streams rather than full listings.

Review/create require completed initial recovery, empty active/legacy capture
folders (including unknown/incomplete entries), no observed contact/failed priority
signal, and matching non-reused local authoring epoch plus exact native generation.
The generation JNI returns decimal text behind the app-authorised Status check,
avoiding JSON numeric precision loss. New authoring/admission/contact changes cannot
silently become covered by an older review. Uncommitted or unknown drafts are
retained and block backup until explicit recovery. Protected legacy capture replay
is tested before backup proceeds; neither raw observations nor unknown records are
discarded by backup.

Reviewed creation takes explicit confirmation, bounded caller password chars,
fresh trusted verification and an explicit encrypted-byte quota. It writes an
owner-only encrypted private file; native sink retirement forces/closes the channel
before key delivery, then the private directory is synced. Prepared explicitly
owns a caller-copyable recovery-key char array; callers must clear deliberate
copies. Internal temporary password/key buffers are cleared on failure, cancellation
and retirement. Staging is not a durable external Backed up acknowledgement.

Prepared publication takes a confirmed trusted absolute local file outside managed
no-backup storage. All destination filesystem checks run on the optional worker.
It copies ciphertext to a new random file in the selected parent, checks exact
length/SHA-256 against emitted ciphertext, forces/closes it, atomically hard-links
a new final name without overwrite, syncs the parent, unlinks the temporary and
syncs again. Existing targets, symlinks, corruption and unsupported destinations
cannot silently replace a file. Local hard-link/directory-sync support is required;
this is not a SAF/provider capability or cloud-synchronisation implementation.

Published is acknowledged only after those local filesystem steps. Failure after
the link returns Uncertain, retains an explicit credential path in the still-owned
prepared artifact, and leaves any final encrypted file for inspection. Cancellation
before publication commit is logical; after entry to commit it returns false and
the worker drains to an outcome. A valid older artifact can publish with the
point-in-time current-authoring flag false. Protection/authentication/containment/
deletion changes invalidate uncommitted private artifacts and key capabilities.
Deliberately copied credentials or already committed publication cannot be revoked.

Close invalidates/cancels optional work and serializes artifact deletion behind its
file work. closeFullyAsync awaits controller retirement before LibraryOwnership is
released, including pre-native and post-native work which core close alone cannot
count. A blocked callback/file worker is not claimed hard-terminated. No unguarded
reader or JSON/model credential/path grant is added. Existing/orphan/unknown staging
files block new creation and remain for explicit recovery. Cleanup errors set
needsAttention. Content-free FailureReason codes guide recovery/retry without
propagating provider or callback diagnostics.

## Exact-source validation

Both validators finished with exit 0, empty source working-tree state and identical
**216 SHA-256 source input hashes** for the revision above. Actual source hashes
were rechecked after completion. Evidence is excluded from its own fingerprint;
full pre-run Git status is retained in each report.

- **294 Rust tests passed**, zero failed/ignored; format, strict workspace/all-target
  Clippy, release build and traceability passed.
- **154 Android tests passed**, zero failures/errors/skips, including **13 new actual
  JNI API-35 Robolectric controller tests**. They cover original source-ink identity,
  known/unknown private recovery, protected replay, stale review, quota/sink failure,
  no-overwrite/integrity checks, relock key retirement, older/current status, uncertain
  commit and ownership during blocked pre-native/sink/post-native/committing workers.
- **17 desktop JNI groups passed before R8 and the same 17 after optimization**,
  including exact generation metadata and closed-handle rejection. These are grouped
  fixtures, not an inferred individual assertion count.
- ARM64/ELF, Android lint, debug/release AAR native/consumer-rule packaging and both
  packaged privacy manifest contracts passed. Device, ART, real provider, power-loss
  durability and latency evidence is not inferred from host/Robolectric checks.
- Traceability retains 94 requirements, 103 inherited clauses and 158 candidates;
  no candidate PASS or whole-requirement closure is inferred. E01 did not recur;
  its cause remains unestablished and its historical follow-up remains open.

See [Rust report](continuation-2026-10-24/android-backup/validation.json),
[Android report](continuation-2026-10-24/android-backup/android-validation.json),
[optimized JNI output](continuation-2026-10-24/android-backup/jni-shrunk-smoke.log)
and [gate output](continuation-2026-10-24/android-backup/backend-gate.json).
Raw logs are archived there and mirrored in latest/. The gate returns expected
exit 2 with only the eight open backend groups plus requirement/inherited semantic
coverage; no stale-source, untested-input, mismatched-report or failed-check reason.

Dirty-source development logs are retained separately. Initial ten/twelve/thirteen
controller runs passed. Adding typed reasons then exposed two failures: standard
future wrappers hid RecoveryRequired as Unavailable. The implementation now unwraps
bounded standard future wrappers before preserving its typed reason; the corrected
thirteen-test run and final complete suite passed. Draft retention remained intact,
and no timeout or assertion was weakened. Early compile warnings/raw tool whitespace
are retained verbatim in development logs; final frozen-source logs establish the
actual accepted checks.

## Remaining backend work

SAF/provider and scheduled backup, persisted backup catalogue/saved-vs-backed-up
status, JNI/platform restore, explicit private/public orphan recovery and crash
cleanup, device durability/authentication/privacy, broader optional scheduling and
bounded semantic restore validation remain open. All other registered backend and
semantic-coverage gaps remain; no production H.I. or SDK/model/licensing approval
is fabricated. Ordered ink and pending/manual conversion acceptance remain intact.

The handover folder remains ignored, untracked and absent from local path history.
Local commit/push guards remain intact. No push was performed.
