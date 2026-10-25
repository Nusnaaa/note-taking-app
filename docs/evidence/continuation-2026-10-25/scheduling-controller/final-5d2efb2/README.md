# Full scheduling-controller Android snapshot verification

Exact5d2efb2, clean implementation inputs. Every report source SHA256 entry
matched the checkout before archiving. Full validate_android.py completed with
319 tests, no failures/errors/skips;26 regular and26 actual R8-optimized desktop
JNI groups; ARM64/ELF, debug/release AAR/consumer rules, manifest and lint pass.
Manifest checks include the BIND_JOB_SERVICE-protected exported backup service,
boot permission, non-exported isolated PDF service, INTERNET removal and disabled
content backup/transfer/cleartext contracts. Final APK/ART/device enforcement is
not inferred. Commands, environment, fingerprints, logs and XML are retained.

Invocation: python3 tools/validate_android.py --sdk /tmp/stem-android-sdk
--jdk /usr/lib/jvm/java-21-openjdk-amd64 --gradle
/tmp/stem-build-tools/gradle-8.9/bin/gradle --online with provisioned Cargo,
Gradle and Android cache environment.

The native full328 evidence is the separate18dd246 snapshot; no Rust input changed
in this Kotlin-only scheduling checkpoint, but this is not a fresh paired
all-source native gate pass. All eight backend gap categories remain OPEN.
Protected scheduling delegation and independent-root dispatch remain required.
No production H.I., actual Keystore/Binder/provider/device timing or physical
interruption acceptance is established by these host tests. The new Rust authority
prototype in /tmp/stem-protected-backup-work is separate and not covered here.
