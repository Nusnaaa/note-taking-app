# Protected scheduled-backup development checks

These are focused development checks from `/tmp/stem-protected-backup-work`, based on 5d2efb2 with the accompanying source hashes. Implementation/test source was byte-matched when promoted. This is not exact clean-commit, device, complete backend or whole acceptance evidence.

Commands used Rust 1.90 via `/tmp/stem-cargo/bin`, offline Cargo dependencies, an isolated `/tmp/stem-protected-backup-target`, JDK 21, Android SDK 35 and Gradle 8.9. `CARGO_HOME=/tmp/stem-cargo RUSTUP_HOME=/tmp/stem-rustup CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=2`; Android uses `/tmp/stem-gradle-cache` and `/tmp/stem-android-user`. Gradle required the host local daemon socket, with escalation after sandbox socket denial.

Passing terminal checks:

- `cargo test -p stem-core --test backup_delegation --locked --offline`: 7 passed, zero failed/ignored. Canonical library descriptor/freshness, private encrypted-only authority with app/note reads still locked, independent restore, wrong scopes/library/options rejection, future authored revisions, off-writer verification and actual authorizer retirement.
- `cargo build -p stem-bridge --locked --offline`: exit 0 after canonical library fix.
- `javac -d target/delegation-jni android/backend/src/main/java/org/stemnotebook/backend/NativeBridge.java bridge/tests/java/{BoundarySmoke,BackupBoundarySmoke,OpenBoundarySmoke,RestoreBoundarySmoke}.java` then `java -Xcheck:jni -Djava.library.path=target/debug -cp target/delegation-jni BoundarySmoke`: exit 0; 28 desktop JNI fixture groups. These are not device/R8 results.
- `gradle -p android :backend:testDebugUnitTest --tests '*NotebookSchedulesTest' --tests '*BackupPolicyMigrationTest' --tests '*BackupSchedulePolicyTest' --tests '*BackupPolicyStoreTest' --tests '*BackupSecretProtectorTest' :backend:lintDebug --offline --no-daemon`: exit 0, 58 passed, zero failures/errors/skips; lint passed.
- Earlier `cargo clippy -p stem-core -p stem-bridge --all-targets --locked --offline -- -D warnings`: exit 0 before the canonical library follow-up; retained only as intermediate evidence, not final-source lint.

Retained failures: initial JNI review rejected the raw SQLite library UUID (32 characters) against the canonical transport UUID form; fixed descriptor normalization and UUID comparison. First Android run had 42 tests/11 failures with that transport. After the UUID correction, the 58-test run had 9 failures caused by Robolectric loading the new nested JNI authority/callback types separately from the parent entrypoint. The final loader fix shares both types and all 58 pass. Production privacy error containment was preserved.

At archive time the full native workspace run and exact-commit Android/ARM64/R8 regression remain pending. Personal-use/device Keystore, actual provider/OS/interruption acceptance, independent-root scheduling dispatch and all eight backend gaps remain open. No production Android H.I. is authorized by these results.
