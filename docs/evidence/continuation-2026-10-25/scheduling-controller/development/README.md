# Scheduling controller/service development verification

Final component revision:55 tests passed, zero failures/errors/skips, plus lint.
All Java/Kotlin source sets, every source byte and the Android manifest/build
inputs were checked against the main checkout after promotion. The final55
snapshot contains11 native workflow,3 service,15 credential-store,15 pool,5 runtime
and6 existing ownership tests. XML, logs and full module input hashes are retained
in final-components-55. No duplicate main-checkout55 run is inferred.

Actual command, using JAVA_HOME=/usr/lib/jvm/java-21-openjdk-amd64,
GRADLE_USER_HOME=/tmp/stem-gradle-cache, ANDROID_HOME=/tmp/stem-android-sdk,
ANDROID_USER_HOME=/tmp/stem-android-user and JAVA_TOOL_OPTIONS=-Duser.home=/tmp/stem-android-user:

    /tmp/stem-build-tools/gradle-8.9/bin/gradle --no-daemon -p /tmp/stem-scheduled-backup-work/android :backend:testDebugUnitTest --tests org.stemnotebook.backend.NotebookSchedulesTest --tests org.stemnotebook.backend.NotebookBackupJobServiceTest --tests org.stemnotebook.backend.BackupPolicyStoreTest --tests org.stemnotebook.backend.BackendOwnershipTest --tests org.stemnotebook.backend.NotebookRuntimeTest --tests org.stemnotebook.backend.SharedNotebookPoolTest :backend:lintDebug --console=plain --offline

Before the final unused-password cleanup API/test and interrupt preservation,
intermediate-broad-119 passed119 tests, no failures/errors/skips, plus lint; it
includes existing local/provider backup and catalogue/JNI regressions and all
policy/migration/platform-adapter tests. Its source hashes/XML/log are retained.
Its command used the same environment/Gradle/-p and test classes:
NotebookSchedulesTest, NotebookBackupJobServiceTest, SharedNotebookPoolTest,
NotebookRuntimeTest, BackendOwnershipTest, BackupPolicyMigrationTest,
BackupJobSchedulerTest, BackupPolicyStoreTest, BackupSchedulePolicyTest,
NotebookBackupsTest, BackupProviderTest, BackupCatalogueTest and
BackupCatalogueIntegrationTest, each supplied with --tests and the complete
org.stemnotebook.backend prefix, followed by :backend:lintDebug --console=plain --offline.
The earlier six-workflow and36-test results are intermediate, not final-source
whole-backend acceptance.

Development failures are retained truthfully: two v1 migration fixtures opened
known files with default Android flags, adding android_metadata and correctly
triggering unknown-schema rejection before migration. Corrected known-file
mutations use NO_LOCALIZED_COLLATORS; the tampered valid envelope additionally
asserts entry into actual AEAD decryption rather than accepting an earlier schema
failure as that proof. A44-test run exposed cancellation observation retiring the
service slot before its opener actually retired. Explicit ownership acknowledgements,
constructor-failure witnesses and pending/lease retirement waits fix that path.
An intermediate runtime-wrapper compile attempt failed on Void future inference;
explicit types were fixed before the passing36/119/55 runs.

These are desktop JNI/Robolectric/JCA and synthetic-provider/permission tests,
not actual Android Keystore, Binder/ART, OS scheduling/device timing, real provider,
physical interruption or complete backend acceptance. Primary public-content
scheduling is integrated; protected-content delegation and independent-root
routing remain required unfinished work. All eight backend gap categories remain
OPEN and no production H.I. is authorized. Full exact-commit Android validation
follows the implementation commit. Native Rust inputs are unchanged; last complete
native328 evidence is the separate18dd246 snapshot.
