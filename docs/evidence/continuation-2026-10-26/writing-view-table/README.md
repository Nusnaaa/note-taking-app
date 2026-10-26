# Writing-view table projection component verification

Base commit d0a22bb, dirty implementation described by ADR-049. `cargo test -p stem-core --test table_cell_ink --test positions --test workspace --test exercises --test objects --test layers` passed 128 cases: table 77, positions 10, workspace 7, exercises 18, objects 8, layers 8; 0 failed/ignored. This precedes extending the existing host route test. The extended `cargo test -p stem-core --test table_cell_ink host_table_structure_command_runs_through_the_writer` then passed 1 case (76 filtered). Its first compile attempt failed because the test imported Camera/ViewSettings from objects rather than store; the original failed log is retained. No production failure or device reproduction is inferred.

Commands used PATH=/tmp/stem-cargo/bin:$PATH, CARGO_HOME=/tmp/stem-cargo, RUSTUP_HOME=/tmp/stem-rustup, CARGO_TARGET_DIR=/tmp/stem-pen-response-target, CARGO_INCREMENTAL=0, CARGO_BUILD_JOBS=2, RUST_TEST_THREADS=2. Initial `cargo check --workspace --all-targets` passed before the final direct canvas visibility guard and test extension; it is not final-source proof.

Full earlier e1adb55 native/Android validation is archived separately. No full current-source, actual device, physical calibration or model SDK acceptance is implied. All backend gaps remain open.

Final `cargo fmt --all -- --check`, traceability regeneration/check, `cargo clippy --workspace --all-targets --locked --offline -- -D warnings` and `cargo build -p stem-bridge --locked --offline` all passed (driver terminal exit 0). The freshly built bridge and JNI-loaded copy have identical SHA-256 bab91c7e52e4be357dd3d744ba0db4a79b5dda8e65e9181cdc7195bc48c4fa63. See final-checks.log.

Desktop JNI driver session 89748 terminated with exit 0: all ten checks passed, including 52 groups before and after fresh R8 processing. The complete component/JNI source maps match all 410 files. The report records dirty source based on d0a22bb; intervening commit 91ab1ce contains only earlier validation evidence. ARM64, Gradle and device tests were not run for this component checkpoint.
