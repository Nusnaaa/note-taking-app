# Exact-source JNI removal and recovery-capacity correction validation

Source `f9814fbe5400067ed9e34c075367a9845401ebdc`. Native and Android input maps match exactly (343 source files). Native command `python3 tools/validate.py --output docs/evidence/continuation-2026-10-26/engine-removal-jni/final-f9814fb/native`: all five stages passed, 450 tests passed with zero failed/ignored. Android command `python3 tools/validate_android.py --gradle /tmp/stem-build-tools/gradle-8.9/bin/gradle`: all nine stages passed, 364 tests with zero failures/errors/skips and both desktop JNI smoke forms passed 43 groups. Commands/environment are retained in reports.

This corrected-source result is separate from the retained d6051d4 native capacity-test failure, and predates transfer-ownership changes. It does not prove device performance, real-model quality/licensing, hard isolation or full backend acceptance. All eight backend gaps remain open.
