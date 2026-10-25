# Explicit protected initial-backup preparation

Isolated development on fcc0716, with source hashes in this directory. `allowProtectedContent=true` is explicit initial encrypted-backup preparation consent, requiring a fresh trusted owner proof for the exact native library plan. It does not install ordinary note access, publish, store credentials or enable a job; enablement retains its separate persistent protected-delegation consent. The default ordinary review path remains unchanged.

Environment: JDK 21, Android SDK 35, offline Gradle 8.9, desktop JNI built from the Rust implementation in fcc0716 using the matching isolated source. Synthetic JCA device keys/provider/job adapters and Robolectric API 35 are not actual device authentication/Keystore/provider acceptance. Gradle uses its approved host local daemon socket.

Commands (both terminal exit 0):

- `gradle -p android :backend:testDebugUnitTest --tests '*NotebookSchedulesTest.freshConsentedPreparationNeedsNoOrdinaryNoteUnlockAndDoesNotRunOnWriter' --tests '*NotebookSchedulesTest.declinedFreshProtectedPreparationCreatesNoPublicationCredentialOrJob' :backend:lintDebug --offline --no-daemon`: 2 tests passed plus lint. Fresh callback re-enters the writer, ordinary note reads stay denied before/after setup and separate persistent consent is required. A declined fresh proof creates no publication/key/job.
- `gradle -p android :backend:testDebugUnitTest --tests '*NotebookSchedulesTest' --tests '*NotebookBackupsTest' --offline --no-daemon`: all recorded controller regression XML suites pass, with totals in `source-sha256.json`.

This is focused development evidence, not a clean-commit full Android/native regression or complete backend acceptance. The exact-commit fcc0716 validation is independent. All backend gaps remain open.
