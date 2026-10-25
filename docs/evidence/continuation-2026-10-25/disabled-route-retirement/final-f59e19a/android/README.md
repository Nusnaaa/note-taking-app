# Exact-source disabled-route retirement Android validation

`python3 tools/validate_android.py --sdk /tmp/stem-android-sdk --jdk /usr/lib/jvm/java-21-openjdk-amd64 --gradle /tmp/stem-build-tools/gradle-8.9/bin/gradle` completed with exit 0 against pushed f59e19a. All tracked non-evidence source hashes matched at archive time. The original owner host-restore evidence directory was untouched.

346 Android/Robolectric tests passed, zero failures/errors/skips. All 28 desktop JNI groups passed normally and after actual R8 release-classfile optimization/minification. ARM64/ELF, debug/release AAR native/consumer-rule packaging, both manifest contracts and lint passed. This includes reviewed disabled-route retirement and the preceding protected/independent/shared-runtime workflow.

The preceding complete native run passed 335 tests plus release, lint, format and traceability on 3b9b0a0. Every Rust/Cargo/migration input is byte-identical here; it is not a new whole-source native validation of f59e19a, whose Kotlin/metadata source differs. Actual device/Keystore/provider/boot/ART and complete backend acceptance remain unestablished. All eight backend gaps remain open and production H.I. remains gated.

Reports retain commands, environment, hashes, timings and original output. Builds used Rust 1.90, cached offline dependencies, NDK 27.0.12077973, JDK 21, Android SDK/build-tools 35 and Gradle 8.9 with its approved host local daemon socket. Pen-pressure implementation remains isolated and outside this verified source.
