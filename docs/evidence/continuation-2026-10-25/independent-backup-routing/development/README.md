# Independent-library backup routing development checks

The isolated implementation was based on local 8c3937c (the tested fresh protected preparation change subsequently promoted in 70f49ff). Runtime source and test files were byte-matched on promotion. Full source hashes and staged component snapshots are retained; status documents and generated traceability maps are reviewed/regenerated separately. These are component runs, not an exact clean-commit full validator or backend acceptance.

Environment: JDK 21, Android SDK 35, offline Gradle 8.9, cached `/tmp/stem-gradle-cache`, `/tmp/stem-android-user` and actual desktop JNI built from the unchanged fcc0716 Rust implementation. Gradle uses its approved host local daemon socket. Framework JobScheduler/provider/device-key observations are synthetic; no actual device/Keystore/boot/ART claim.

All listed terminal runs exited 0:

- `gradle -p android :backend:compileDebugUnitTestKotlin --offline --no-daemon`: initial dispatcher compilation.
- Component selectors `*BackupJobRoutesTest`, `*BackupJobSchedulerTest`, `*NotebookBackupJobServiceTest`, `*NotebookRuntimeTest`, `*SharedNotebookPoolTest`: intermediate runs, followed by 40 passing tests plus `:backend:lintDebug`.
- `*NotebookBackupJobServiceTest.mappedIndependentJobOpensOnlyItsExistingLibraryAndRecordsCredentialFailureThere`: 1 test, actual native library selection and durable unavailable-credential outcome in the independent library; primary notebook absent. This precedes strengthening the test to share a foreground lease.
- The five component selectors plus `*NotebookSchedulesTest` and `*NotebookBackupsTest`, with lint: 69 passing tests before strict integer/bounded-reader and recovered-acquisition follow-up. Its XML/source snapshot is explicitly intermediate.
- `*BackupJobRoutesTest` plus `*NotebookRestoresTest.recoveredLibrarySharesForegroundLeasesAndNeverCreatesAMissingCopy`, with lint: 10 tests passed after the reader and shared recovered API changes.
- The five component selectors plus all `*NotebookRestoresTest`, with lint: 60 tests passed, zero failures/errors/skips. This covers unchanged legacy exclusive-open/recovery behavior alongside the new shared API.
- `*NotebookBackupJobServiceTest` plus `*NotebookRuntimeTest`: 11 tests passed after strengthening the mapped-job test to share an already-open foreground runtime, preserve its writer after service retirement and release ownership only after the last lease.

Each command uses `:backend:testDebugUnitTest --tests '<selector>'` for every selector and `--offline --no-daemon`; logs retain tool output. Final full Android/ARM64/JNI/R8/packaging validation is still pending at this checkpoint. The Rust source is unchanged from the 335-test run archived under protected-scheduling; its stale generated-map failure was retained and corrected separately. All eight backend gaps and the production H.I. gate remain open. Inactive/abandoned root route retirement and actual device/provider/Keystore/boot acceptance remain required.
