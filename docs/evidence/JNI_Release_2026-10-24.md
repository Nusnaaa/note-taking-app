# JNI release consumer rules and optimized boundary — 24 October 2026

Validated source: `f5e8622fa5635e63748005c33698fc894c897818`, branch
`phase-a/backend`. Backend acceptance remains **NOT MET**; production H.I.
remains gated. This resolves the consumer-rule/reporting follow-up recorded with
[the JNI backup transport increment](JNI_Backup_2026-10-24.md).

## Implemented

The Android backend now packages consumer ProGuard/R8 rules in both debug and
release AARs. JNI exported names, JNI-created nested constructors, callback
interfaces and concrete Verification/PdfGrant/BackupOutput methods retain the names
native code calls. Unrelated backend code remains eligible for shrinking.
Existing JVM fixtures now exercise concrete verifier classes as well as lambdas.

The Android validator compiles fresh classes in a temporary build directory, runs
the full actual-JNI boundary, then optimizes/minifies/tree-shakes those classes
using the already-installed SDK 35 build-tools R8 (8.6.2-dev). Only fixture main is
kept by test configuration; the actual consumer rules must retain JNI members.
The mapping must show BackupBoundarySmoke renamed, and the same native smoke
groups must pass under `-Xcheck:jni` in the optimized artifact. No compiler download
or application SDK/model dependency was added.

Both debug/release AAR builds now run. Their packaged consumer rules must exactly
match tracked rules, their ARM64 libraries must match, and both packaged manifests
must retain the privacy merge contract. The gate now requires optimized JNI and
consumer-rule evidence. Smoke group totals are parsed from actual PASS output;
the obsolete fixed `jni_assertions: 10` field is removed. Tool/version/artifact
hashes, mapping and commands are retained. No ZIP/JAR bundle is committed.

## Frozen-source validation

Both runners terminated with exit 0 and finished timestamps, empty source working
tree and identical **214 source SHA-256 hashes** for the source revision above.
All current source hashes were rechecked after completion. Evidence is excluded
from its own fingerprint; full pre-run Git status is retained.

- **294 Rust tests passed**, zero failed/ignored; formatting, strict all-target
  Clippy, workspace release build and traceability passed.
- **141 Android tests passed**, zero failed/errors/skipped; lint, ARM64/ELF,
  debug/release AARs and both packaged manifest contracts passed.
- **17 JNI smoke groups passed before R8 and the same 17 after R8**. These are
  grouped fixtures, not individual assertion counts. Existing protection/Unicode/
  registry/close checks and backup key-array, ciphertext, cancellation, callback
  failure and retirement checks run in both artifacts.
- The backend gate returns expected exit 2 with only the eight open backend groups
  and requirement/inherited semantic coverage. No stale-source, new untested-input,
  source-mismatch, missing-check or release-packaging failure remains.
- Traceability retains 94 requirements, 103 inherited clauses and 158 candidates;
  no candidate PASS or whole-requirement closure is inferred. E01 did not recur;
  its original cause remains unestablished and its follow-up remains open.

See [Rust report](continuation-2026-10-24/jni-release/validation.json),
[Android report](continuation-2026-10-24/jni-release/android-validation.json),
[R8 mapping](continuation-2026-10-24/jni-release/jni-shrinker.map),
[optimized native output](continuation-2026-10-24/jni-release/jni-shrunk-smoke.log)
and [gate output](continuation-2026-10-24/jni-release/backend-gate.json).
Raw logs are archived in that directory and mirrored in latest/. Development
checks are retained separately under `jni-release/development/`.

## Remaining scope

This is real desktop R8/classfile/JVM execution and debug/release Android-library
packaging evidence. It does not establish final APK/ART/device enforcement or a
shrunk production app. No production Activity/Compose interface is introduced.
Original ordered-ink persistence, protected backup verification, deliberate proposal
acceptance and privacy exclusions remain unchanged and their regression suites pass.

Next implementation is the NotebookBackend-owned optional backup controller:
serialized pending/private capture-draft barriers, authoring/capture epoch binding,
worker ownership through pre-admission and post-native I/O, quota/sync and deliberate
destination/publication. JNI restore, broader scheduling/semantic validation/crash
cleanup and all other open backend gaps remain. Design notes are recorded in
[next integration](continuation-2026-10-24/jni-release/next-integration.md), not
claimed as implemented or tested capability. No model/licensing approval is invented.

The handover folder remains ignored, untracked and absent from local path history.
Local commit/push guards remain intact. No push was performed.
