# Complete Android verification for JNI discard

All nine Android validator checks completed against clean non-evidence source 37923d2: 366 API-35 simulated tests with zero failures/errors/skips; 46 desktop JNI groups before/after R8 release classfile optimisation; ARM64 native compilation/linking; debug/release assembly, lint, consumer rules and manifest/AAR policies. Every source hash matches 37923d2 and the prior component source map (356 files). No APK/ART/device or real recogniser acceptance is inferred.

Full native 474-test proof for aecb885 precedes the JNI increment; it is not a matching-source pair for this report. The subsequent SDK resource-reservation work is absent from both source versions and needs its own evidence. All broader backend gaps remain open.
