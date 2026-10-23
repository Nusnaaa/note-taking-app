First exact-source Android attempt for 1bff36f: ARM64/ELF/JNI passed; Gradle failed with one of 114 tests timing out.
The XML stack locates the five-second timeout in PdfRetentionTest.fixture's initial Create command (line 64), before selection or retention starts. No block read or receipt comparison is invoked by this initial Create.
Cause is not established. This failure is retained, not counted as a pass. The named case is being rerun independently to investigate setup/worker delay before another complete validator run.
