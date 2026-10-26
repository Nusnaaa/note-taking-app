# Structural table cut and canvas-scope component checks

Executed in `/tmp/stem-engine-catalogue-work` with PATH=/tmp/stem-cargo/bin:$PATH, CARGO_HOME=/tmp/stem-cargo, RUSTUP_HOME=/tmp/stem-rustup, CARGO_TARGET_DIR=/tmp/stem-pen-response-target, CARGO_INCREMENTAL=0, CARGO_BUILD_JOBS=2 and RUST_TEST_THREADS=2 for tests.

`cargo test -p stem-core --test table_cell_ink --test editing --test layers --test exercises --test objects` passed the four non-table suites (41 cases), then ended with69 table passes and1 fixture failure. The privacy fixture tried to store syntactically invalid JSON and SQLite's existing json_valid CHECK refused the setup before the tested API ran. The corrected fixture uses valid JSON with an invalid Embed shape (`{}`). `cargo test -p stem-core --test table_cell_ink` then passed all70 cases,0 failed/ignored. Production Rust code is unchanged between these runs:111 distinct passing cases across the five selected suites; the initial combined command remains failed and is retained as such.

Cases cover source-hidden/destination-independent table cuts, same-master one-entry Undo and cross-master atomic linked Undo/Redo, exact original source retention, commit rollback, source/destination scope/revision checks, encrypted restore and redo-branch retirement. Actual host JSON tests invoke MoveTable and Undo. Canvas cases cover source membership, old/current layer state, independent raw-source queries, embed overrides and revisions, camera rotation/zoom/frame composition, source-point preservation, parent protection, and denied malformed embed payloads through direct and scene reads. Earlier66-table move pass predates scoped-scene and shared embed-read changes and is retained separately.

`cargo fmt --all -- --check`, source/doc `git diff --check`, and `python3 tools/traceability.py --check` passed. All94 requirements/103 inherited clauses/158 candidates remain retained; no candidate acceptance is inferred.

ADR047 records structural-cut identity/reference semantics, retained source/history, coupled journal rules and single-embed scene scope. Outer view/multi-ancestor composition, follow-object references, complete rendered derivatives and selection workflows remain open. Response and numeric geometry checks are not physical-device or latency SLOs. Native13/schema22 stay unchanged; all eight backend gaps remain open.

Matching full575-native/380-Android evidence for earlier sourceac45d44 is archived separately under table-copy-vector-pages/final-ac45d44. It predates this implementation and does not establish full final-source acceptance for table cuts/scoped pages.

`cargo clippy --workspace --all-targets -- -D warnings` and `cargo build -p stem-bridge --locked --offline` passed. Fresh shared-library hashes match the JNI runtime copy; no binary is committed.

`python3 tools/validate_jni.py --output docs/evidence/continuation-2026-10-26/table-cut-canvas-scope/jni --r8 /tmp/stem-android-sdk/build-tools/35.0.0/lib/d8.jar` terminated exit0: all10 checks passed;52 aggregate desktop JNI groups passed before/after fresh R8 classfile optimization. All 406 current source hashes exactly match the component map. Dirty baseac45d44, not clean full-source, ART/device or Android-lifecycle acceptance.
