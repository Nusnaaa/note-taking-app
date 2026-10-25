# Scheduled-backup foundations — component verification

No schedule or automatic job is enabled by this increment. Durable policy/secret
storage and the actual scheduler/service still require implementation.

The initial secret primitive passed 11 tests with Android lint in an isolated
source/build copy. The initial combined permission/secret primitives passed 19;
reviewed epoch coverage passed 21; explicit cancellation/interruption coverage
passed 24. These are development states, not additive executed-test totals.
Provider/permission adapters and key providers are synthetic; AEAD executes real
JCA AES-GCM. Android platform adapter sources compile/lint, but no Android
Keystore, real persisted provider grant, hardware backing or device behavior is
claimed. Commands and XML are retained, with earlier stages separately labelled.

Four tested files were applied byte-for-byte from the isolated 24-test pass.
Main-checkout review moved plaintext allocation inside the key-lease try/finally
so allocation failure cannot leave the lease acquired. The main regression then
passed 68 tests: 13 secret, 11 persistent-access, 13 catalogue, 5 catalogue/JNI,
13 local-backup/JNI and 13 provider-backup/JNI. No failures/errors/skips; lint pass.
root-inputs.json identifies the final changed source/test files.

Command (provisioned JDK21, Gradle8.9, Android35; Gradle/Android caches under /tmp):

    gradle --no-daemon -p android :backend:testDebugUnitTest --tests org.stemnotebook.backend.BackupSecretProtectorTest --tests org.stemnotebook.backend.BackupPersistentAccessTest --tests org.stemnotebook.backend.BackupCatalogueTest --tests org.stemnotebook.backend.BackupCatalogueIntegrationTest --tests org.stemnotebook.backend.NotebookBackupsTest --tests org.stemnotebook.backend.BackupProviderTest :backend:lintDebug --console=plain --offline

Isolated commands used -p /tmp/stem-scheduled-backup-work/android. Both use
JAVA_HOME=/usr/lib/jvm/java-21-openjdk-amd64, GRADLE_USER_HOME=
/tmp/stem-gradle-cache, ANDROID_HOME=/tmp/stem-android-sdk, ANDROID_USER_HOME=
/tmp/stem-android-user, JAVA_TOOL_OPTIONS=-Duser.home=/tmp/stem-android-user.

Traceability regeneration/check and git diff --check pass. No requirement,
source candidate or whole-backend acceptance is promoted. These Kotlin-only
foundations do not change native code. The previous complete native/catalogue
proof remains separately archived for 6d11241; full affected Android validation
follows on this committed snapshot. No fresh full native run is claimed here.
