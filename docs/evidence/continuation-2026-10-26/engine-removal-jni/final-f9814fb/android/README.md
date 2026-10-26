# JNI removal and capacity-correction source Android validation

Source `f9814fbe5400067ed9e34c075367a9845401ebdc`. `python3 tools/validate_android.py --gradle /tmp/stem-build-tools/gradle-8.9/bin/gradle`: all nine stages passed, 364 tests, zero failures/errors/skips; both desktop JNI smoke forms passed 43 groups. Commands, environment and 343 input hashes are retained. This source predates transfer ownership locks; its full native validation remains running separately.

Desktop JNI/API-35 simulation does not establish actual device performance, real-model rights/quality, or complete backend acceptance. All eight backend gaps remain open.
