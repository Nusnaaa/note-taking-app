Development-only, dirty-source checks (not frozen-source acceptance):

- First JVM smoke stopped at BackupBoundarySmoke line 29 because the new fixture incorrectly expected includesOriginalInk=true for an empty library. The summary correctly reports actual stored ink, not merely the inclusion option. Fixed the fixture to expect false; production logic unchanged. This first stdout/stderr was observed in the tool response rather than redirected to a file.
- The next attempt failed because its synthetic capture session was not a UUID; retained in stem-backup-jni-smoke.log. Replaced the fixture ID with a UUID.
- The following JVM run passed seven backup groups and the existing ten groups under -Xcheck:jni; retained in stem-backup-jni-smoke-2.log.
- The first Android development suite and lint passed in 2m43s with the first five backup tests. Its temporary log was subsequently reused for the final development run; it is not exact-source evidence. Final six-test, malformed UTF-16 and reentrant-close coverage is validated in the frozen-source runs separately.

No source requirement/candidate PASS or device evidence is inferred from these development checks. Existing E01 remains open.
