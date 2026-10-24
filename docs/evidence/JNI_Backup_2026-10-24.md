# Trusted JNI backup transport — 24 October 2026

Validated source: `bc7d27c41ce2519597116bd9063f8be9e3482542`, branch
`phase-a/backend`. Backend acceptance remains **NOT MET** and production H.I.
remains gated. This is a backend increment, not an app-level backup workflow.

## Implemented boundary

Typed AutoCloseable BackupReview/BackupJob capabilities bind the exact non-reused
host identity and cannot be constructed from notebook JSON. Reviews expose actual
ink inclusion, retained-source count and omission metadata; one current native job
is reserved per host. Stale insertion cannot evict a newer review, closed hosts
cannot resurrect registry admission, foreign native tokens cannot consume an
owner's review, and cancellation retains running entries until actual completion.
Registry locks never span Java verification/output callbacks or native close drain.

The worker-only JNI path runs existing native authoritative pulls/encryption off
the writer. Trusted output callbacks receive ciphertext frames bounded to 65,552
bytes. Explicit confirmation, current protected review scope and fresh trusted
verification remain native requirements. Capture invalidation never revives old
review authority. Callback exceptions are cleared and replaced with generic errors;
write, verification and retirement failure do not expose a recovery key. Admitted
native work retains exclusive ownership through actual output retirement, including
reentrant close without self-joining.

Passwords are bounded before Java cloning/native allocation. Temporary Java char
arrays and Rust UTF-16/decoded buffers are cleared, including malformed-surrogate
rejection within already-zeroizing storage. The caller retains and clears its own
password. Only success after sink retirement exposes a caller-owned recovery-key
char array, never a Java String or ordinary JSON value. The caller must clear it.
No credential, recovery key, handwriting upload, SDK/model or production H.I. was
introduced. Existing native roundtrips continue to preserve original ordered ink.

BackupOutput is a trusted resource owner: retire must close all owned resources
before returning even on failure. The Java wrapper makes one retirement attempt,
with fallback on pre-native rejection. Logical cancellation is not hard termination
of a blocked callback, and external resource clones are outside the contract.

## Validation

Both validators terminated with exit 0 and finished timestamps, identical source
commit and **212 matching SHA-256 input hashes**, and empty source working-tree
state. Evidence is excluded from its own fingerprint; full pre-run status remains
in each report. All commands and captured output are archived in
[the increment directory](continuation-2026-10-24/jni-backup/validation.json) and
[Android report](continuation-2026-10-24/jni-backup/android-validation.json).

- Rust: **294 passed**, zero failed/ignored; format, strict workspace/all-target
  Clippy, workspace release build and traceability passed.
- Android: **141 passed**, zero failed/errors/skipped; six new actual-JNI API-35
  Robolectric tests cover ciphertext/key/single-use, protected access/fresh proof/
  confirmation, foreign token isolation, bounded/malformed secrets and callback
  failure, reentrant callback close, and blocked-retirement ownership/writer progress.
- Desktop JVM `-Xcheck:jni`: **seven backup groups plus the existing ten groups**
  passed, covering cancellation, capture invalidation, callback failure/retirement,
  independent registry access and close ownership. The validator's legacy
  `jni_assertions: 10` field still counts its original groups; the expanded smoke
  output is authoritative for the additional seven groups. Reporting cleanup is
  recorded with the release-packaging follow-up; no assertion count is inferred.
- ARM64 build/ELF, lint, AAR native packaging and merged-manifest exclusions passed.
  No device, real provider durability, shrunk release app or recogniser quality
  evidence is implied. E01 did not recur; its cause remains unestablished and open.
- Traceability retains 94 requirements, 103 inherited clauses and 158 candidates;
  no candidate PASS or whole-requirement completion is inferred.

The backend gate returns its expected exit 2 with only the eight open gap groups
and requirement/inherited semantic coverage. There are no stale, untested-input,
source-mismatch, missing-report or failed-check reasons. See
[gate output](continuation-2026-10-24/jni-backup/backend-gate.json).

Development fixture failures/corrections and final development Android output are
retained in `jni-backup/development/`. Empty-library ink inclusion and capture UUID
fixture errors were corrected without changing production logic or weakening
assertions.

## Remaining implementation

NotebookBackend still needs a bounded dedicated optional-worker controller,
review/reconciliation of active/legacy private recovery drafts, trusted destination
quota/sync/publication, JNI restore transport and ownership through all work rejected
before native admission or continuing after native completion. Native close alone
only counts admitted core work. Committed SQLite export must not silently claim to
include private journals or durable public publication.

JNI-reflected classes/members also need packaged consumer R8 keep rules and actual
shrunk-boundary validation before a release app; the current module has no consumer
rules. This is the next packaging increment. Details are recorded in
[release follow-up](continuation-2026-10-24/jni-backup/release-follow-up.md).

Initial metadata preparation, bounded semantic restore validation, staging disk/
crash cleanup, broader scheduling and all other open backend gaps remain. No model
installation or licensing approval is fabricated. Production H.I. remains gated.

`handover-do-not-commit/` remains ignored, untracked and absent from local path
history; executable local commit/push guards remain intact. No push was performed.
