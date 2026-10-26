# Table source-layout component verification

The implementation adds optional explicit source-unit layout and per-stroke cell origins, ink-aware sized row/column edits and resizing, source-preserving derived placement, cell-local recognition vectors and native13/schema21 compatibility fences. ADR043 records semantics and remaining work. All eight backend gaps remain open.

Executed with PATH=/tmp/stem-cargo/bin:$PATH, CARGO_HOME=/tmp/stem-cargo, RUSTUP_HOME=/tmp/stem-rustup, CARGO_TARGET_DIR=/tmp/stem-pen-response-target, CARGO_INCREMENTAL=0, CARGO_BUILD_JOBS=2 and RUST_TEST_THREADS=2 for tests.

`cargo test -p stem-core --test table_cell_ink --test workspace` passed 40 cases (33 table, 7 workspace), 0 failed/ignored, on final geometry/source checks and the corrected streaming header. Tests include reflow without source rewriting, independent capture origins before/after movement, original retention and Undo on cell removal, explicit legacy adoption, finite bounds, stale proposal/protection refusal, native copy/history gates, both encrypted restore formats and schema20-header refusal. These are synthetic source-unit contract checks, not device performance or recogniser quality.

Earlier nine regression suites passed 61 cases: authoring, host_conversion, interrupted_capture, layers, objects, positions, pressure_response, recovery and stroke_style. Their retained combined log ends with the new streaming-layout test failure. Those 61 passes predate the final stream-header and placement-finiteness checks; they are not presented as a successful full run or as final-source acceptance.

Retained failures: a missing Content qualification during development; a test calling nonexistent import preparation methods (corrected to the actual import_native API); a native writer expectation still asserting v12 (including an unsuccessful first fixture correction); and a production streaming writer still emitting schema20. The writer was corrected to schema21, after which the new stream restore/downgrade test passed. Initial 30-table pass predates the new backup cases.

Full matching 81d22e3 evidence is archived separately under local-cell-identity/final-81d22e3. Its 533 native / 380 Android cases and matching 390-file maps predate this implementation and do not prove this newer source. Complete scene/export integration, cell move/copy, resource/malformed coverage and backend acceptance remain open.

Additional final-source commands passed:

- `cargo test -p stem-core --lib backup::validation::tests::schema_feature_gates_match_aggregate_oracle -- --exact`: 1 passed, 0 failed/ignored.
- `cargo clippy --workspace --all-targets -- -D warnings`: exit 0.
- `cargo build -p stem-bridge --locked --offline`: exit 0. The freshly built shared library was copied to the JNI runtime path and both hashes match (native-library-sha256.json); no binaries are committed.
- `python3 tools/traceability.py --check`: exit 0, all canonical counts retained.
- `cargo fmt --all -- --check`: passed as the JNI driver's first check.
- `git diff --check -- core docs/decisions docs/formats docs/verification`: exit 0. Raw evidence preserves compiler formatting.

All 395 current component source-file hashes equal the JNI driver's complete source map, including new files. This is a dirty component snapshot based on e6159de, not a claim of clean full acceptance.

`python3 tools/validate_jni.py --output docs/evidence/continuation-2026-10-26/table-source-layout/jni --r8 /tmp/stem-android-sdk/build-tools/35.0.0/lib/d8.jar` terminated exit 0. All ten checks passed; all 52 aggregate desktop JNI smoke groups passed both unshrunk and after fresh R8 classfile optimization. This is not ART/device or Android lifecycle acceptance.
