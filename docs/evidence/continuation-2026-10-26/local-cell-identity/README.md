# Scoped cell identity component verification

Native: 157 tests in 18 selected suites, none failed or ignored; this includes 20 table-cell cases. Android: 50 actual desktop JNI/API35 component tests, no failures/errors/skips, plus lintDebug. A repeated-cell-ID case captures both distinct tables independently. All ten JNI validator checks passed: 52 aggregate groups before and after fresh R8 optimisation. Workspace all-target Clippy denied warnings and traceability passed. Runtime/Cargo native-library hashes match.

The staged 390-file non-evidence source map exactly equals the JNI component map, including untracked implementation/tests/docs at invocation. Initial stale-lock, private-encoder and guarded-edit fixture failures remain retained separately; they are not counted as passing runs. All observations are synthetic app-owned data, not a device/model-quality benchmark.

Complete native/Android validation of this source remains separate. The 8829963 complete proof archived under android-cell-capture/final-8829963 predates this fix. Table sizing/row/column workflows, scene fidelity and broader backend acceptance remain open; no binaries or model weights are committed.
