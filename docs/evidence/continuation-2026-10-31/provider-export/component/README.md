# Provider export component checks

Dirty implementation based on1099b44. This is not complete backend acceptance. Root source changes add the provider adapter, export worker, private attempt catalogue and backend lifecycle hooks; Rust/JNI protocol source remains unchanged.

`gradle -p android :backend:testDebugUnitTest --tests org.stemnotebook.backend.NotebookExportsTest --offline --no-daemon --max-workers=2` passed all6 original publication tests. The later focused command adds imports, ownership and admission: `--tests ...NotebookExportsTest --tests ...NotebookImportsTest --tests ...BackendOwnershipTest --tests ...BackendAdmissionIntegrationTest`, same Gradle flags. It passed37 tests (8/8/6/15), zero failures/errors/skips. Exact logs and selected current TEST XML are preserved separately; the first six are included in the later eight and are not additional distinct tests.

Environment: pinned Gradle8.9/JDK21/SDK35, persistent `target/tooling/gradle-cache` and `target/tooling/android-user`; `JAVA_TOOL_OPTIONS=-Duser.home=<absolute repository>/target/tooling/android-user`. Actual desktop JNI and ARM64 files are the previously verified current builds; no Rust/Java native-boundary source changed for this component. This is API35 simulation with synthetic providers and host descriptors, not device/Binder/cloud durability evidence.

The first compile failed on a copied worker initializer referring to `operationId` instead of this review's `id`; `initial-compile-failure.log` is retained. It was corrected before passing tests. After the focused pass, the owned password lifetime was shortened to end immediately after native encryption; full regression checks are running on that final source and will be recorded independently. No unfinished command is counted as successful.

Complete catalogue recovery/resource coverage remains open: no paged attempt discovery yet, and whole-record JSON updates require further normalization/boundary checks. Existing native/derivative scene and PDF-linked-source restrictions remain inherited. All eight backend gaps remain OPEN.
