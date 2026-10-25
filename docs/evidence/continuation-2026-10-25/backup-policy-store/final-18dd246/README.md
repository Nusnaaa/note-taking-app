# Complete policy/vault snapshot verification

Both full runners identify exact18dd246, clean implementation inputs. Their full
source SHA256 manifests are identical and were checked against the unchanged
checkout before archiving.

Native: 328 passed, none failed/ignored; format, workspace/all-target clippy,
workspace tests, release build and traceability pass.
Android: 274 tests, no failures/errors/skips; 26 regular and26 actual R8-optimized
desktop JNI groups; ARM64/ELF, debug/release AAR, consumer rules, manifest and
Android lint pass. Actual commands, environments, fingerprints and outputs are
retained in the two reports and associated logs/XML.

Native command: python3 tools/validate.py with provisioned Cargo/Rust paths.
Android command: python3 tools/validate_android.py --sdk /tmp/stem-android-sdk
--jdk /usr/lib/jvm/java-21-openjdk-amd64 --gradle
/tmp/stem-build-tools/gradle-8.9/bin/gradle --online, with provisioned Cargo,
Gradle and Android cache environment. backend_gate.py exits2 for only the eight
open backend categories and incomplete requirement/inherited-clause coverage;
there are no stale-source/mismatch reasons at this snapshot.

No complete backend acceptance, production H.I., actual device/provider/Keystore
or physical interruption acceptance is inferred. Runtime, trusted scheduling
controllers and JobScheduler/service execution still need implementation. The
shared-runtime prototype under /tmp is separate and not covered by this snapshot.
