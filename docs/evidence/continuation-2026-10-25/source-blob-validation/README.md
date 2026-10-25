# Incremental retained-source asset validation

Whole-library restore now hashes retained source assets through SQLite's read-only
incremental BLOB API with a 64 KiB Rust buffer. It no longer asks rusqlite to return
an entire source asset as Vec<u8> during semantic verification. SQLite retains
ownership of database access. No raw database file descriptors are opened.

The existing pinned rusqlite version now enables its `blob` feature; no dependency
version or license policy changes. Digest, retention authority, export consent,
asset class and source identity validation remain mandatory. The streaming
retained-source roundtrip fixture now crosses three chunk boundaries; these are
synthetic bytes, not a PDF renderer or device performance acceptance test.

Limits remain open: aggregate NativeDocument graph/image validation, inherited
subset count/image caps, complete streaming cell allocations, cancellation inside
semantic validation, and total SQLite/scratch allocation budgets. This change
establishes no total memory, disk or elapsed-time bound. Backend completion and
requirement coverage gates remain open. Existing latest full-suite evidence
predates this change and does not validate these inputs.

Commands use PATH=/tmp/stem-cargo/bin:$PATH, CARGO_HOME=/tmp/stem-cargo,
RUSTUP_HOME=/tmp/stem-rustup and CARGO_BUILD_JOBS=2.

- `cargo test -p stem-core --lib source_asset_validation_tests`: 2 passed,
  0 failed. Includes empty assets, boundary lengths, corruption in final chunks,
  non-BLOB storage and missing rows.
- An initial development build failed because a broad edit accidentally changed
  the unrelated image asset decoder's byte column to i64. Corrected before tests
  passed. An intermediate substring-based implementation also passed 22 existing
  backup tests; it was replaced with SQLite incremental BLOB reads. Those earlier
  results are not evidence for the final BLOB implementation.

Final incremental BLOB implementation verification:

- `cargo test -p stem-core --test backup --test backup_stream --test backup_pull`:
  22 passed (9 legacy, 5 pull, 8 streaming), no failures/ignored. Includes the
  existing encrypted archive exceeding the legacy 64 MiB format limit and the
  enlarged retained-source fixture.
- `cargo clippy -p stem-core --lib -- -D warnings`: exit 0.
- `cargo fmt --all -- --check`: exit 0.
- `git diff --check`: exit 0 for source and this evidence.

Command output is retained beside this note. No new full Android, JNI, release or
entire-workspace validation is claimed for this checkpoint.
