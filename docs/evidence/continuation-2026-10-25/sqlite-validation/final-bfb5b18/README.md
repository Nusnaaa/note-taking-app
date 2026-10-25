# Complete SQLite-validator checkpoint verification

Both full runners identify source bfb5b18dacd3af89f1246be02918bf5949099e74
with clean implementation inputs. Source SHA256 manifests are identical and were
checked against current files before archiving. This supersedes the intermediate
20a24f1 results for the corrected validator/source-retirement/checkpoint code.

Native: 328 passed, no failed/ignored; format, workspace/all-target clippy, full
workspace tests, release build and traceability passed.
Android: 207 tests, no failures/errors/skips; 26 regular and R8-optimized desktop
JNI groups; ARM64 build/ELF, debug/release AAR consumer-rule and manifest policies,
unit/Robolectric tests and lint passed.

Commands and observed outputs are retained verbatim in this directory. Environment
and exact commands are in validation.json and android-validation.json. Native
command: python3 tools/validate.py with the provisioned Rust/Cargo paths. Android
command: python3 tools/validate_android.py --sdk /tmp/stem-android-sdk --jdk
/usr/lib/jvm/java-21-openjdk-amd64 --gradle
/tmp/stem-build-tools/gradle-8.9/bin/gradle --online with the provisioned Cargo,
Gradle and Android user-cache environment.

backend_gate.py exits 2 with the eight open backend gaps and incomplete whole
requirement/inherited-clause coverage. No stale/mismatching validation reason is
present. Backend acceptance and production H.I. remain blocked by incomplete
implementation/coverage; these tests establish component correctness, not device,
real-provider, power-loss or empirical pen/model acceptance.

Backup-catalogue prototype work under /tmp is separate and is not covered by this
checkpoint. It must receive its own integration and exact-source verification.
