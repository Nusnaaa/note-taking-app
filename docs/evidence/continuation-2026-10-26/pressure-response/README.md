# Explicit pressure response and eraser preferences — component evidence

Uncommitted implementation based on f59e19a; the source hashes identify the precise native inputs. No backend acceptance or device performance claim. All eight backend gaps remain open.

`workspace.log`: `cargo test --workspace --locked --offline`, exit 0, 342 passed, none failed or ignored. Rust/Cargo/migration inputs remained frozen through completion. `workspace-source-sha256.json` was recorded during the run before the later eraser-underflow correction.

`android-regressions.log` and `android-broad/`: 99 Android unit/Robolectric tests plus lint passed using rebuilt desktop JNI after schema-16 strict-opener correction. Selectors covered InkAccumulator, MotionCapture, ActiveCaptureJournal, JournaledCaptureIntegration, BackendAdmissionIntegration, NotebookRuntime, NotebookBackups and NotebookRestores. These precede the final eraser-underflow check.

`journal-tests.log` records a development tooling failure: overlapping Gradle invocations in the same checkout raced test-result files. The later serialized capture run passed 31 tests plus lint (`capture-serialized.log`, `capture/`); no pass is inferred from the failed invocation. Other logs are intermediate checks and do not certify the later whole source.

The legacy schema-15 streaming test is a synthetic authenticated legacy-layout fixture made from pressure-free rows; it is not output from a historic executable. Missing pressure is distinct from measured zero. Eraser gesture hit-testing, physical calibration and full nib/scene fidelity remain open.

Final correction: reject a derived eraser width of zero after source-unit multiplication. `underflow-tests.log`: `cargo test -p stem-core --test pressure_response --locked --offline`, exit 0, 8 passed. `underflow-clippy.log`: `cargo clippy --workspace --all-targets --locked --offline -- -D warnings`, exit 0. The earlier whole-workspace run predates this one validation condition and its added regression test. A subsequent exact-commit validator is required for the complete final source.
