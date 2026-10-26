# Nested table ink scene component checks

Commands executed in `/tmp/stem-engine-catalogue-work` using PATH=/tmp/stem-cargo/bin:$PATH, CARGO_HOME=/tmp/stem-cargo, RUSTUP_HOME=/tmp/stem-rustup, CARGO_TARGET_DIR=/tmp/stem-pen-response-target, CARGO_INCREMENTAL=0, CARGO_BUILD_JOBS=2 and RUST_TEST_THREADS=2 for tests.

- `cargo check --workspace --all-targets`: exit0.
- `cargo test -p stem-core --test table_cell_ink --test layers --test objects`:89 passed (73 table,8 layers,8 objects),0 failed/ignored.
- `cargo test -p stem-core --lib store::table_scene::budget_tests::reserved_envelope_budget_shrinks_fragments_without_skipping_observations -- --exact`:1 passed,0 failed/ignored.
- `cargo clippy --workspace --all-targets -- -D warnings`:exit0.
- `cargo fmt --all -- --check`, source/doc `git diff --check`, `python3 tools/traceability.py --check`:passed. All94 requirements/103 inherited clauses/158 candidates retained; no candidate PASS inferred.

Tests compose three camera/frame levels, retain rotated clip rectangles plus suffix transforms, preserve original contact points, recheck parent epochs and outer protection, reject disconnected paths, honour intermediate base-layer overrides and final-host visibility, and exercise an actual host JSON nested-page request. An internal reduced-wire-budget case reconstructs every source point and continuity sample after adaptive fragment shrinking. These are synthetic logical-coordinate contract checks, not device/performance/SLO acceptance. No failed test attempt occurred in this component run.

Path/clip metadata shares the existing scene byte budget. Parent clips are not flattened into AABBs, source observations are not clipped or rewritten, and the traversal is iterative over an explicit supplied path. Native13/schema22 remain unchanged. Outer writing-view/screen/flow transforms, automatic follow-object references, rendered derivatives and selection workflows remain open; all eight backend gaps remain open. ADR048 records the current semantics.

Full e1adb55 native/Android validators run independently on earlier source. Their results cannot establish acceptance of this newer nested-path implementation.

`cargo build -p stem-bridge --locked --offline` passed; the fresh shared-library runtime copy matches its Cargo hash. No binary is committed.

`python3 tools/validate_jni.py --output docs/evidence/continuation-2026-10-26/nested-table-scene/jni --r8 /tmp/stem-android-sdk/build-tools/35.0.0/lib/d8.jar` terminated exit0: all10 checks passed and all52 aggregate desktop JNI groups passed before/after fresh R8 classfile optimization. All 408 current source hashes exactly match its complete map. This is a dirty component snapshot based one1adb55, not clean full-source, ART/device or Android-lifecycle acceptance.
