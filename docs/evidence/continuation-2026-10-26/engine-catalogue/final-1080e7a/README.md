# Exact-source native catalogue/default validation

Source `1080e7ae13e25fe78eea609a50e3280243b0040b`; all 332 source hashes match the archived Android run. `python3 tools/validate.py --output docs/evidence/continuation-2026-10-26/engine-catalogue/final-1080e7a/native`: all five stages passed, 434 tests passed with zero failed/ignored. `python3 tools/validate_android.py --gradle /tmp/stem-build-tools/gradle-8.9/bin/gradle`: all nine stages passed, 359 tests with zero failures/errors/skips and both desktop JNI smoke forms passed 39 groups. Commands/environments are retained in reports.

This source includes native Store/Host catalogue and durable defaults, predating their JNI/Android adoption and read-only recovery. It does not establish device performance, real model rights/quality or full backend acceptance; all eight backend gaps remain open. Latest mutable evidence is not overwritten while a newer Android validator is active.
