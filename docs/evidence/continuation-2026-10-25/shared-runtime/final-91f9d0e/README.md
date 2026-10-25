# Full shared-runtime Android verification

Exact91f9d0e, clean implementation inputs; every source SHA256 entry matched the
checkout before archiving. Full validate_android.py completed: 292 tests, zero
failures/errors/skips; 26 regular and26 actual R8-optimized desktop JNI groups;
ARM64/ELF, debug/release AAR/consumer rules/manifest and lint pass. Commands,
environment, fingerprints, logs and all test XML are retained here.

Invocation: python3 tools/validate_android.py --sdk /tmp/stem-android-sdk
--jdk /usr/lib/jvm/java-21-openjdk-amd64 --gradle
/tmp/stem-build-tools/gradle-8.9/bin/gradle --online with provisioned Cargo,
Gradle and Android cache environment.

Portable core/bridge/Cargo/toolchain input fingerprints are unchanged from the
complete native328 test evidence at18dd246, archived separately. That full native
suite was not repeated for this Kotlin-only change; this is not a fresh paired
all-source native/backend gate pass. All eight backend gap categories stay OPEN.
The scheduling controller/OS service and protected-content scheduling authority
remain unimplemented. No real device/provider/Keystore, physical interruption or
production H.I. acceptance is inferred.

This snapshot predates the isolated policy-v2/scheduler prototype and corrected
SQLite mutation fixtures under /tmp. Those source changes and their results are
not covered by the292-test snapshot.
