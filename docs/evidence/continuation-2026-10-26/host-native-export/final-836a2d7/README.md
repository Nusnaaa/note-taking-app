# Complete validation of 836a2d7

Native driver 41418 and Android driver 8135 both terminated with exit 0 and finished report timestamps. Native: all five checks PASS, 606 passed / 0 failed / 0 ignored. Android: all nine checks PASS, 380 tests / 0 failures / 0 errors / 0 skipped; 55 JNI groups passed before and after fresh R8. Both source admissions are clean and their complete 418-file source maps match exactly.

Native command: `python3 tools/validate.py --output docs/evidence/continuation-2026-10-26/host-native-export/final-836a2d7/native`, with CARGO_INCREMENTAL=0, CARGO_BUILD_JOBS=2, RUST_TEST_THREADS=2. Android command: `python3 tools/validate_android.py --sdk /tmp/stem-android-sdk --jdk /usr/lib/jvm/java-21-openjdk-amd64 --gradle /tmp/stem-build-tools/gradle-8.9/bin/gradle`. Commands and environments remain in the reports/logs. Only current check logs, report, shrinker map and TEST XML were copied from Android latest; unrelated historical attempt files are excluded.

This validates 836a2d7 only, excluding later derivative snapshot/host/JNI work. Android uses Robolectric API 35 and desktop JVM JNI/R8, not device tests or final APK enforcement. Backend acceptance remains false; all eight backend gaps remain open. Evidence-only dirty/untracked paths are recorded by admission and do not change source matching.
