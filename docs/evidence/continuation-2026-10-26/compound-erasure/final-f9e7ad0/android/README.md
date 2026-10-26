# Exact f9e7ad0 Android backend validation

All checks passed: ARM64 native/ELF, 28 ordinary JNI groups and the same 28 under actual R8 release-classfile optimisation, debug/release AAR assembly and consumer-rule/packaged manifest contracts, 351 API-35 unit/Robolectric tests with no failures/errors/skips, and lint. Precise source hashes, commands, logs and XML are retained.

This is desktop JNI, API-35 simulation and library AAR evidence; ART/device and final APK enforcement remain separate. The same-source full native validator also passed all checks and 353 tests; its source hash map exactly matches this Android run. See the adjacent native archive. All eight backend gaps remain open; no complete backend acceptance or performance SLO is inferred.
