# Native recovery-source Android validation

Source `bc00d4589a4276a6cb12fe5aefc2958eed0d4ce6`. `python3 tools/validate_android.py --gradle /tmp/stem-build-tools/gradle-8.9/bin/gradle` passed all nine stages: 360 tests, zero failures/errors/skips, desktop JNI and R8 smoke each passed 39 groups under this source's validator policy. Commands/toolchain details and all 337 input hashes are retained. This source predates JNI recovery (which adds the 40th group) and transactional removal. Native full validation of this exact source remains active separately.

Desktop JNI/API-35 simulation does not establish device performance, real-model rights/quality or complete backend acceptance. All eight backend gaps remain open.
