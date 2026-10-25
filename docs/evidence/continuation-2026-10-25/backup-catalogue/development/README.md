# Backup publication catalogue development verification

The five Kotlin implementation/test files were developed in an isolated Android
source/build copy while bfb5b18 full validation kept unchanged implementation
inputs. The reviewed 43-test/lint pass was applied byte-for-byte to the main
checkout. Main-checkout review then made the sequence explicitly AUTOINCREMENT
and added exhaustion/no-wrap coverage. The main run passed 44 tests (13 catalogue,
5 actual-JNI integration, 13 local-backup, 13 provider-backup), with no failures,
errors or skips; Android lint passed. root-inputs.json records final changed inputs.

The isolated initial catalogue-only run passed 9 tests with Kotlin warnings.
Warnings were corrected before integrated runs. An initial integrated run had
39 tests with one failed new fixture: it tried SetAppLock without the required
App authentication. The backend correctly denied it. The fixture now authenticates
App, enables the lock and relocks before testing history privacy. This was not a
production authorisation change. Failed output/XML is retained under earlier-attempts.
Subsequent 39-, 41- and 43-test/lint passes cover successive development states;
only root-tests-lint.log represents the final main-checkout 44-test stage.

Commands used the provisioned JDK21, Gradle8.9/Android35, GRADLE_USER_HOME=
/tmp/stem-gradle-cache, ANDROID_USER_HOME=/tmp/stem-android-user and
JAVA_TOOL_OPTIONS=-Duser.home=/tmp/stem-android-user. Main command:

    gradle --no-daemon -p android :backend:testDebugUnitTest --tests org.stemnotebook.backend.BackupCatalogueTest --tests org.stemnotebook.backend.BackupCatalogueIntegrationTest --tests org.stemnotebook.backend.NotebookBackupsTest --tests org.stemnotebook.backend.BackupProviderTest :backend:lintDebug --console=plain --offline

The isolated command used -p /tmp/stem-backup-catalogue-work/android and a separate
JNI artifact copy (hash recorded), rather than changing the frozen main runner.

`cargo test -p stem-core --test converters offline_help_covers_recovery_and_unsearchable_ink_without_remote_dependency`: one passed.
Java NativeBridge plus all four smoke-fixture sources compiled with JDK21 javac
into /tmp/stem-catalogue-javac. The new no-library Help assertion has not yet been
claimed executed in that targeted step; full JNI validation follows. Rust format
and traceability checks pass. Whole backend/candidate acceptance remains open.
These are synthetic host/JNI/Robolectric checks, not device/provider or physical
power-loss acceptance. No backup credential or notebook payload is test evidence.

Workspace/all-target `cargo clippy --workspace --all-targets --locked --offline -- -D warnings` passed after the Help change. `cargo fmt --all -- --check` and regenerated `python3 tools/traceability.py --check` passed. The map was regenerated after adding sequence-exhaustion coverage; the initially stale-map check was corrected without changing any acceptance status.
