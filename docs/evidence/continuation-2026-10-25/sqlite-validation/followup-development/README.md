# Isolated follow-up development checks

These checks ran in /tmp/stem-sql-edge-followup-work with a separate target
directory, while both full runners kept source snapshot 20a24f1 unchanged.
The three corrected Rust files are pending application to the main checkout.
They remove populated CREATE INDEX sorting and pass legacy worker cancellation
through table ingestion and semantic validation. This is development evidence,
not full validation of the corrected Git snapshot or device acceptance.

Environment: PATH=/tmp/stem-cargo/bin:$PATH, CARGO_HOME=/tmp/stem-cargo,
RUSTUP_HOME=/tmp/stem-rustup, CARGO_TARGET_DIR=/tmp/stem-sql-edge-followup-target,
CARGO_BUILD_JOBS=1, CARGO_PROFILE_DEV_DEBUG=0, CARGO_PROFILE_TEST_DEBUG=0.

- cargo test --manifest-path /tmp/stem-sql-edge-followup-work/Cargo.toml -p stem-core --locked --offline --lib embed_work_queries_use_covering_primary_key_without_temporary_sorting: 1 passed.
- cargo test --manifest-path /tmp/stem-sql-edge-followup-work/Cargo.toml -p stem-core --locked --offline --lib backup::validation::tests: 10 passed, no failures/ignored.
- cargo clippy --manifest-path /tmp/stem-sql-edge-followup-work/Cargo.toml -p stem-core --all-targets --locked --offline -- -D warnings: exit 0.

The work-table correction follows inspection of pinned SQLite sqlite3RefillIndex
(OP_SorterOpen/OP_SorterInsert) and vdbeSorterOpenTempFile (VFS temporary journal).
This establishes an actual possible sorter path; no fixture disk spill is claimed.
A host Python SQLite 3.45.1 plan probe was exploratory only. The retained Rust test
executes the actual pinned bundled SQLite and checks covered primary-key edge
search, no temporary B-tree in traversal plans and negative/zero cursor ordering.

## Main-checkout correction verification

After both intermediate full runners completed, the three isolated Rust files
were applied and checked against the isolated source hashes. Further review in
the main checkout removed the source-grant retirement table-wide Vec, added
transactional cancellation/identity coverage and replaced raw open-stage fsync
with a checked SQLite-owned WAL checkpoint. These subsequent changes are covered
by root-final-source-inputs.json, not the earlier isolated manifest.

Main checkout commands use the same Cargo/Rust paths with CARGO_BUILD_JOBS=2
and the normal workspace target directory/debug settings.

- cargo test -p stem-core --lib backup::validation::tests: 12 passed, no failures/ignored.
- cargo clippy --workspace --all-targets --locked --offline -- -D warnings: passed.
- cargo fmt --all -- --check: exit 0.
- python3 tools/traceability.py --check: exit 0; no requirement/candidate PASS inferred.
- git diff --check: exit 0 for changed source/docs before adding raw command logs.

An earlier main-checkout eleven-test run passed before the final checkpoint
correction. A separate initial checkpoint test also passed before adding explicit
non-WAL/no-op rejection; the final twelve-test run covers the complete correction.
Full native/Android/JNI validation will run against the committed corrected
snapshot. The intermediate 324-native/207-Android results are archived separately
and must not be claimed as exact-source acceptance for these corrected inputs.
