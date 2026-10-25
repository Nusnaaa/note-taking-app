# Disabled-route retirement component verification

Development on 3b9b0a0. Runtime source/test files were byte-matched when promoted; status documents and generated traceability maps are reviewed/regenerated separately. Environment: offline Gradle 8.9, JDK 21, Android SDK 35, Robolectric API 35, cached test dependencies and actual desktop JNI from the unchanged Rust source in 3b9b0a0. Gradle uses its approved host local daemon socket. Synthetic framework jobs/JCA test keys do not establish actual device/Keystore/boot behavior.

All commands exited 0:

- `gradle -p android :backend:compileDebugUnitTestKotlin --offline --no-daemon`.
- `:backend:testDebugUnitTest` with selectors `*BackupJobRoutesTest`, `*BackupJobSchedulerTest`, plus `:backend:lintDebug`: 22 tests passed. Slot reuse, stale callback isolation, foreign/uncertain mapping preservation, live-policy changes and post-commit lost acknowledgement are covered; lost acknowledgement is conservatively Uncertain even when the deletion committed.
- Selector `*NotebookSchedulesTest.reviewedDisabledRouteRetirementPreservesPolicyKeysHistoryAndBackupFiles`: 1 test passed, using actual native initial encrypted backup and synthetic platform job registration. Enabled policy, declined proof and missing consent cannot retire a route. After disablement, retirement preserves policy revision/history, credential/key metadata and the external backup; a second retirement reports Absent.
- Selectors `*BackupJobRoutesTest`, `*BackupJobSchedulerTest`, `*NotebookSchedulesTest`, `*NotebookBackupsTest`, `*NotebookBackupJobServiceTest`, `*NotebookRuntimeTest`, `*SharedNotebookPoolTest`, plus `:backend:lintDebug`: 77 tests passed, zero failures/errors/skips.

All test commands use `-p android --offline --no-daemon` and `--tests` for each listed selector. Original logs and XML are retained. This is component evidence, not a new clean-source full validator. Prior 3b9b0a0 matched-source validation passed all 335 native and 339 Android tests, with all 28 normal/R8 JNI groups and packaging/manifests/lint. Full combined verification remains pending. Abandoned/missing-root recovery, actual device/provider/boot acceptance and all eight backend gaps remain open; production H.I. remains gated.
