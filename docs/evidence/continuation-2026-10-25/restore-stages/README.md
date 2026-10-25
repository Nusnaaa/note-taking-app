# Reviewed unfinished restore retirement — 25 October 2026

Implementation source: `3d91741`. The complete stable-source native and Android
runs passed with identical source SHA-256 manifests and clean implementation
inputs: 314 native tests, zero failures/ignored tests; 207 Android tests, zero
failures/errors/skips; 26 JNI fixture groups before and after R8. Formatting,
workspace Clippy, release build, ARM64, lint, AAR and manifest checks passed too.
Original commands/results are retained in validation.json, android-validation.json
and adjacent logs. The gate still exits 2 for the eight unfinished implementation
groups and canonical/inherited-clause coverage, with no stale-source or mismatched
run reason. See backend-gate.json. The original backend goal remains active.

The implementation adds exact private purpose/library markers, bounded opaque
stage review, fresh access/proof/confirmation, private quarantine of unclassified
items and permanent discard only of identified scratch, protocol-only records or
exact empty private recovery folders. Metadata/root identities are revalidated;
payloads are never opened as raw SQLite FDs. Possible effects after transfer keep
an uncertain receipt through cancellation/close and can be inspected after restart.
Original working-library files and private capture journals are outside this
cleanup namespace. See RESTORE_STAGES.md and ADR-006 for actual contracts/limits.

Offline Help now has a read-only host query and a bounded no-library JNI entrypoint
so recovery guidance remains available without opening a damaged library. It
explains permanent discard, content not necessarily in the current library,
preserved quarantine and uncertain outcomes. Static guidance is available while
app content is locked, without changing generation or invalidating a valid review.

Executed development checks:

- 14 RestoreStagesTest cases passed; nine RestoreStageLifecycleTest cases passed;
  lint passed. Their actual XML and selected-run logs are retained in development/.
- Earlier combined commands passed with the existing 17 NotebookRestoresTest
  cases; retained XML distinguishes those source checkpoints from the later strict
  marker/no-library guidance changes.
- The new native locked-app/read-only Help test passed. Workspace Clippy passed.
- The actual desktop JNI BoundarySmoke command passed all 26 fixture groups,
  including bounded Help with no library handle. The final runner also retained
  normal and R8-optimized output; an earlier desktop run alone is not R8 evidence.
- The first child-JVM lifecycle attempt failed on a null Robolectric CodeSource
  location before the child ran. Resource-URL classpath discovery corrected that
  harness issue; the next nine-case run passed, including real abrupt JVM exit
  after native staging. The failed attempt's original XML remains retained.

Development Android command patterns (JDK/SDK/Gradle caches under /tmp, no device):

```
JAVA_HOME=/usr/lib/jvm/java-21-openjdk-amd64 GRADLE_USER_HOME=/tmp/stem-gradle-cache ANDROID_HOME=/tmp/stem-android-sdk ANDROID_USER_HOME=/tmp/stem-android-user JAVA_TOOL_OPTIONS=-Duser.home=/tmp/stem-android-user /tmp/stem-build-tools/gradle-8.9/bin/gradle --no-daemon -p android :backend:testDebugUnitTest --tests org.stemnotebook.backend.RestoreStagesTest --tests org.stemnotebook.backend.RestoreStageLifecycleTest :backend:lintDebug --console=plain
PATH=/tmp/stem-cargo/bin:$PATH CARGO_HOME=/tmp/stem-cargo RUSTUP_HOME=/tmp/stem-rustup CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0 cargo test -p stem-core --test host offline_recovery_help_is_readonly_and_available_while_app_locked --locked --offline
javac -d /tmp/stem-stage-jni-classes android/backend/src/main/java/org/stemnotebook/backend/NativeBridge.java bridge/tests/java/*.java
java -Xcheck:jni -Djava.library.path=target/debug -cp /tmp/stem-stage-jni-classes BoundarySmoke
```

These are synthetic filesystem/provider, host-JVM and desktop-JNI checks, not
ART/provider/device power-loss acceptance. The child JVM probe is developer-only.
Host Unix-attribute tests do not establish the Android lstat fallback on a device.
Hard native staging allocation quotas, bounded graph-validation memory, complete
orphan salvage/export, actual eligible recognition and the other recorded backend
areas remain unfinished. No production H.I. or backend acceptance is claimed.
