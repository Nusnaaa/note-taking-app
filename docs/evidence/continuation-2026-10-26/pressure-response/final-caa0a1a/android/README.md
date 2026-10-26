# Exact caa0a1a Android backend validation

All validator checks passed: ARM64 release native library/ELF; 28 ordinary and 28 actual R8 release-classfile desktop JNI groups; debug/release AAR assembly, consumer rules and both packaged manifest contracts; 349 API-35 Android unit/Robolectric tests, zero failures/errors/skips; lint. Commands, source hashes and captured output are retained. No ART/device/provider durability, final APK enforcement or backend acceptance is claimed.

The separate exact-source native workspace run failed a synthetic backup sink-start timeout and its unchanged targeted follow-up passed. Both are retained in the adjacent native directory; this Android success does not replace full native verification. All eight backend gaps remain open.
