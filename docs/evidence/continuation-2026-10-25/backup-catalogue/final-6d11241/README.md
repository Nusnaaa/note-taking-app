# Full backup-catalogue snapshot verification

Both full runners identify source 6d11241, with clean implementation inputs.
Their complete source SHA256 manifests are identical and were checked against
current files before archiving. The scheduled-secret prototype under /tmp is
separate and is not covered by this snapshot.

Native: 328 passed, none failed/ignored; format, workspace/all-target clippy, full
workspace tests, release build and traceability pass.
Android: 225 tests, no failures/errors/skips; 26 regular and R8-optimized desktop
JNI groups; ARM64/ELF, debug/release AAR/consumer rules and manifest, Android lint
and all unit/Robolectric tests pass. The new no-library BackupStatus Help
assertion executes in both JNI runs; no new fixture group is inferred.

Actual commands, outputs, environment and full source fingerprints are retained
beside this note in validation.json and android-validation.json plus their logs.
Native invocation: python3 tools/validate.py with provisioned Cargo/Rust paths.
Android invocation: python3 tools/validate_android.py --sdk /tmp/stem-android-sdk
--jdk /usr/lib/jvm/java-21-openjdk-amd64 --gradle
/tmp/stem-build-tools/gradle-8.9/bin/gradle --online with provisioned Cargo, Gradle
and Android user-cache environment.

backend_gate.py exits 2 for the eight open backend gaps and incomplete whole
requirement/inherited-clause coverage, with no stale/mismatched source reason.
This does not establish complete backend acceptance, production H.I., real-device
or real-provider/physical interruption acceptance. Scheduled backups and native
allocation controls still require implementation.
