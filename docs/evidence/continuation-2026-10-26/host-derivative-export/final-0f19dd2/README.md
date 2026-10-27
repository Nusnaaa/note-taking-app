# Complete validation of 0f19dd2

Native driver 85055 and Android driver 26759 both terminated with exit 0 and finished report timestamps. Native: all five checks PASS, 617 passed / 0 failed / 0 ignored. Android: all nine checks PASS, 380 tests / 0 failures / 0 errors / 0 skipped; 59 JNI groups passed before and after fresh R8. Both source admissions are clean, and all 425 source-map entries match exactly.

Native command: `python3 tools/validate.py --output docs/evidence/continuation-2026-10-26/host-derivative-export/final-0f19dd2/native`, CARGO_INCREMENTAL=0, CARGO_BUILD_JOBS=2, RUST_TEST_THREADS=2. Android command: `python3 tools/validate_android.py --sdk /tmp/stem-android-sdk --jdk /usr/lib/jvm/java-21-openjdk-amd64 --gradle /tmp/stem-build-tools/gradle-8.9/bin/gradle`. Complete commands/environments are in the reports/logs. Android archive includes only current check logs/report/map and TEST XML, excluding historical attempts and binaries.

This validates 0f19dd2 (schema22/native13), excluding later schema23/native-import receipt and delivery work. Robolectric API35/desktop JNI/R8 and ARM64 build are not physical-device tests or final APK enforcement. All eight backend gaps remain open and backend_accepted remains false. Known evidence-only dirty/untracked paths are retained in admission reports and do not alter source matching.
