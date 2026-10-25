# Shared runtime component verification

Final main-checkout run: 24 tests (13 pool, five actual-JNI/Robolectric runtime,
six existing ownership), zero failures/errors/skips; Android lint passed. The
same final24 and lint passed in isolation before copying. XML, logs and source
SHA256 inputs are retained in final-root-24 and final-isolated-24.

Actual main command with provisioned JAVA_HOME=/usr/lib/jvm/java-21-openjdk-amd64,
GRADLE_USER_HOME=/tmp/stem-gradle-cache, ANDROID_HOME=/tmp/stem-android-sdk,
ANDROID_USER_HOME=/tmp/stem-android-user, JAVA_TOOL_OPTIONS=-Duser.home=/tmp/stem-android-user:

    /tmp/stem-build-tools/gradle-8.9/bin/gradle --no-daemon -p android :backend:testDebugUnitTest --tests org.stemnotebook.backend.SharedNotebookPoolTest --tests org.stemnotebook.backend.NotebookRuntimeTest --tests org.stemnotebook.backend.BackendOwnershipTest :backend:lintDebug --console=plain --offline

The isolated final invocation used the same command/environment with -p
/tmp/stem-scheduled-backup-work/android. It includes the main close/release signal
changes. intermediate-23 is the earlier passing revision before separating open
and retirement workers and adding the slow-open retirement-starvation fixture;
its source hashes are relative to the isolated android/backend directory.
The initial pool-only10 passed/lint. The first runtime attempt had a JUnit class
initialization error: an expression-bodied test returned an exception object;
its runtime integration tests did not execute. The declaration was corrected
before the passing23/24 runs. Raw intermediate logs are kept as development
history and are not final-source/whole-backend acceptance.

Full Android validation follows the implementation commit. This is a Kotlin-only
change; complete native328 evidence is the separate18dd246 snapshot, not a fresh
all-source run for the new Kotlin inputs. No JobScheduler service, automatic job,
production H.I., actual provider/Keystore/device or physical interruption
acceptance is inferred. All eight backend gap categories remain OPEN.
