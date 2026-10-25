# Backup credential/policy component development evidence

The final-components-25 snapshot passed all 25 tests (15 credential/consent-store,
10 schedule-state), no failures/errors/skips, plus Android lint. The six new
source/test files were checked byte-for-byte against the main checkout; hashes
are retained. The isolated checkout also contains the main credential primitive
allocation-finally fix. This is component evidence, not complete backend proof.

Actual invocation, with provisioned JAVA_HOME=/usr/lib/jvm/java-21-openjdk-amd64,
GRADLE_USER_HOME=/tmp/stem-gradle-cache, ANDROID_HOME=/tmp/stem-android-sdk,
ANDROID_USER_HOME=/tmp/stem-android-user and JAVA_TOOL_OPTIONS=-Duser.home=/tmp/stem-android-user:

    /tmp/stem-build-tools/gradle-8.9/bin/gradle --no-daemon -p /tmp/stem-scheduled-backup-work/android :backend:testDebugUnitTest --tests org.stemnotebook.backend.BackupPolicyStoreTest --tests org.stemnotebook.backend.BackupSchedulePolicyTest :backend:lintDebug --console=plain --offline

intermediate-root-83 passed 83 tests and lint in the main checkout BEFORE adding
schedule configuration/run tables and atomic referencing-policy retirement.
It covers the initial store plus secret/grant primitives, catalogue/JNI and
manual local/provider backup regression. Its own source hashes/XML/log are
retained; it is not presented as final-schedule regression evidence.
Its invocation used the same environment and Gradle, -p android, with --tests
for BackupPolicyStoreTest, BackupSecretProtectorTest, BackupPersistentAccessTest,
BackupCatalogueTest, BackupCatalogueIntegrationTest, NotebookBackupsTest and
BackupProviderTest, followed by :backend:lintDebug --console=plain --offline.

Earlier isolated outputs are deliberately retained as intermediate development:
11 tests passed but lint failed on formatting; the first bounded-blob query then
failed nine of 14 tests because NULL intents were changed into malformed blobs.
The corrected initial store passed 15 tests/lint. The first schedule revision
passed 24 tests/lint before the final target-type and active-run retirement test.
Do not infer final-source coverage from those intermediate outputs.

No production controller, shared runtime or OS job/service is enabled by this
change. Actual Android Keystore/provider, device/power-loss and whole-requirement
acceptance remain unexecuted/incomplete. The eight backend gap categories stay
OPEN. Full exact-snapshot validation follows the implementation commit.
