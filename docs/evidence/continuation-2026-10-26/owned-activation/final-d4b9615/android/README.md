# Exact d4b9615 Android backend validation

All checks passed: ARM64 native/ELF; 28 ordinary and the same 28 actual R8 release-classfile JNI groups; debug/release AAR assembly, consumer rules and packaged manifest contracts; 351 API-35 unit/Robolectric tests with no failures/errors/skips; lint. Source remained frozen. Commands, source hashes, logs and XML are retained.

This includes the cropped-image and owned-activation core source, without enabling a Java provider or installing models. It is desktop JNI/simulated API-35/library AAR evidence; actual device/final APK enforcement remains separate. The same-source native validator passed all checks and 365 tests; its source hash map exactly matches this Android run. See the adjacent native archive. All eight backend gaps remain open.
