# Exact-source independent-root Android validation

`python3 tools/validate_android.py --sdk /tmp/stem-android-sdk --jdk /usr/lib/jvm/java-21-openjdk-amd64 --gradle /tmp/stem-build-tools/gradle-8.9/bin/gradle` completed with exit 0 against pushed 3b9b0a0. All tracked non-evidence source hashes matched the checkout when archived. The owner untracked host-restore evidence directory was untouched.

339 Android/Robolectric tests passed, with zero failures/errors/skips. All 28 desktop JNI groups passed normally and after actual R8 release-classfile optimization/minification. ARM64/ELF, debug/release AAR native/consumer-rule packaging, both manifest contracts and lint passed. This includes private independent-root job routing, shared foreground/job runtime, shared recovered-library acquisition and the current encrypted-only protected backup workflow. No actual device/Keystore/provider/boot/ART or full backend acceptance is inferred. The exact native validator remained in progress at archive time. All eight backend gaps remain open and production H.I. remains gated.

The report/logs retain invocation, environment, source hashes and limitations. Builds used Rust 1.90, offline cached dependencies, NDK 27.0.12077973, JDK 21, Android SDK/build-tools 35 and Gradle 8.9 with the approved host local daemon socket.
