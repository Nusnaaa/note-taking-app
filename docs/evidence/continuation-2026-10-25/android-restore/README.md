# Android recovered-copy ownership — 25 October 2026

Source `c3173f4` implements a bounded optional restore controller owned by
NotebookBackend, deliberate encrypted read input, actual cancellation/descriptor
retirement, aggregate-only private review, fresh confirmed separate-root
publication, bounded persistent catalogue discovery and explicit independent
opening. Current capture journals and the healthy working library remain separate.

Final ARM64, desktop JNI, R8 optimization, all Android unit tests, lint and debug/
release AAR/manifest checks passed on that unchanged committed source. Results:
184 Android tests, zero failures/errors/skips; 25 fixture groups before and after
R8. `android-validation.json` records the exact command arguments, source hashes,
clean source status and component results. Newly added controller cases are 17 of
those 184 tests; group counts are not additional Android test counts. This is
synthetic-provider/Robolectric API-35 and desktop-JVM evidence, not ART, real
provider, device responsiveness, physical power-loss or final APK acceptance.

The runner command was executed with the pinned Rust toolchain, Cargo cache,
Gradle 8.9 and Android SDK/NDK under /tmp, using the installed JDK 21:

```
PATH=/tmp/stem-cargo/bin:$PATH CARGO_HOME=/tmp/stem-cargo RUSTUP_HOME=/tmp/stem-rustup GRADLE_USER_HOME=/tmp/stem-gradle-cache ANDROID_USER_HOME=/tmp/stem-android-user JAVA_TOOL_OPTIONS=-Duser.home=/tmp/stem-android-user python3 tools/validate_android.py --sdk /tmp/stem-android-sdk --jdk /usr/lib/jvm/java-21-openjdk-amd64 --gradle /tmp/stem-build-tools/gradle-8.9/bin/gradle --online
```

Review corrected an unsafe raw-FD header preflight before push. Activation now uses
SQLite-owned strict existing-only opens after app ownership admission; it rejects
missing/empty/foreign files before migration instead of initializing them. Three
new native unit tests and the actual JVM open-boundary fixture passed, including
legacy migration and duplicate-owner lock preservation. The 17 controller cases
also check app/native rejected duplicates followed by successful owner edits.
See ADR-006 and HOST_INTEGRATION.md for contracts and exact limitations.

The final full Rust runner is still pending at this evidence checkpoint. Do not
combine the earlier 310-test development attempt with this snapshot and call it
one exact-source full run. Its logs are retained in development/initial-whole-run:
source changed during that attempt, so it is explicitly not final acceptance.
Final Rust evidence will follow after the live validation process retires.

Development: the first 11-case controller run had three failures. Two exposed
Pending.await returning before actual cancelled input retirement; it now waits
for the terminal operation result. The other compared changing response generation
metadata with ink; the fixture now compares source payloads. Corrected 14- and
15-case runs and lint passed; subsequent stage/verification/duplicate tests joined
the final whole run. Their original XML/logs are retained in development/.
Initial Gradle sandbox socket denial was rerun with required escalation. Tool setup
archives are outside Git. Pre-existing latest evidence was preserved separately
before replacement; the 24 October host-restore folder remains untouched.

No complete-backend or production-H.I. acceptance is claimed. Explicit orphan
review/cleanup, hard staging disk quotas, bounded whole-graph semantic validation,
real-provider/device acceptance and the other recorded backend gaps remain open.
