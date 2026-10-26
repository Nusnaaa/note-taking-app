# ee37a6f validation — native failed, Android passed

Input maps match exactly (345 source files). Native validation failed during the core hash-cancellation fixture (36 library cases passed, one failed) after format/Clippy passed; it did not complete later suites, release or final traceability. Its report/logs remain preserved. Follow-up e2436d4 explicitly releases the original transfer owner and passed all 37 core library tests, recorded separately.

`python3 tools/validate_android.py --gradle /tmp/stem-build-tools/gradle-8.9/bin/gradle` passed all nine stages: 364 tests, zero failures/errors/skips; both desktop JNI forms passed 43 groups. This source predates discard/schema17. Android success does not replace the failed native command or prove device/model-rights/quality acceptance. All eight backend gaps remain open.
