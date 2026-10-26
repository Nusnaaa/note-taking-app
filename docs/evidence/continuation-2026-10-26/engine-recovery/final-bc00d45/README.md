# Exact-source native recovery validation

Source `bc00d4589a4276a6cb12fe5aefc2958eed0d4ce6`. Native and Android source maps match exactly (337 inputs). `python3 tools/validate.py --output docs/evidence/continuation-2026-10-26/engine-recovery/final-bc00d45/native`: all five stages passed, 443 native tests, zero failed/ignored. `python3 tools/validate_android.py --gradle /tmp/stem-build-tools/gradle-8.9/bin/gradle`: all nine stages passed, 360 tests, zero failures/errors/skips, both JNI smoke forms passed 39 groups under this source's validator policy. Commands and environments remain in the reports.

This source predates JNI recovery and transactional removal. It does not establish device, real-model rights/quality or full backend acceptance; all eight backend gaps remain open. Latest mutable evidence is not overwritten while the newer Android validator is active.
