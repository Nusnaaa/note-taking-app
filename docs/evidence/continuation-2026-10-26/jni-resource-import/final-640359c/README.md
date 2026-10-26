# Exact-source JNI resource import validation

Source: `640359cfb71c4995888a79e1d495c1eac2aec16a`. Native and Android source manifests match exactly (329 files).

Commands: `python3 tools/validate.py --output docs/evidence/continuation-2026-10-26/jni-resource-import/final-640359c/native`; `python3 tools/validate_android.py`. Environment and individual commands/results are retained in reports.

All five native checks passed: 429 passed, zero failed/ignored. All nine Android checks passed: 359 tests, zero failures/errors/skips; desktop JNI smoke and shrunk smoke each passed 39 groups. These do not establish device performance, recognition quality, or model licensing. Backend acceptance remains open pending the tracked gaps.
