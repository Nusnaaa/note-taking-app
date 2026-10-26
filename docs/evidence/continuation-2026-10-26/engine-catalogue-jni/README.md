# JNI and Android engine catalogue/default integration

Commands and results:

- `cargo build -p stem-bridge --locked --offline`: passed; copied desktop library to the isolated project's `target/debug` for actual JNI execution.
- `cargo test -p stem-bridge --lib --locked --offline`: 2 passed, zero failed/ignored.
- `cargo clippy --workspace --all-targets --locked --offline -- -D warnings`: passed after moving JNI exports before the Rust test module; initial lint failure retained.
- `cargo fmt --all -- --check`, `python3 tools/traceability.py --check`, `git diff --check`: passed.
- JDK 21 `javac` compiled NativeBridge and EngineBoundarySmoke; `java -Xcheck:jni -Djava.library.path=target/debug -cp target/catalogue-smoke EngineBoundarySmoke`: passed, with new assertions for metadata-only staged state, invalid cursors/limits, unloaded/unsupported default rejection, loaded catalogue readiness, persisted task metadata and null-override inference. Existing fixture groups retain their count.
- R8 8.6.2 release classfile shrinking with the production consumer rules and fixture main entry, then `java -Xcheck:jni -Djava.library.path=target/debug -cp target/catalogue-shrunk.jar BoundarySmoke`: passed all 39 groups. Mapping confirms EngineBoundarySmoke renamed. The initial command incorrectly supplied a directory as R8 input; the retained unsupported-source-type failure was corrected by supplying a JAR. No JAR or binary is committed.
- Cached Gradle 8.9 `--offline --no-daemon -p android :backend:testDebugUnitTest --tests org.stemnotebook.backend.NotebookEnginesTest :backend:lintDebug --console=plain --max-workers=2`: 7 tests, zero failures/errors/skips; lint passed. Initial 7-test run had one fixture failure because the expected-error assertion ran outside the observer and saw Future.get's ExecutionException wrapper. Moving the assertion inside the observer fixed the fixture; initial log/XML retained.

Offline Rust cache, CARGO_INCREMENTAL=0, CARGO_BUILD_JOBS=2, target /tmp/stem-pen-response-target, default debug. Android SDK /tmp/stem-android-sdk and JDK /usr/lib/jvm/java-21-openjdk-amd64; Gradle cache /tmp/stem-gradle-cache; no actual device run. These synthetic providers prove lifecycle/API behavior, not real recognition quality or model licensing. Expected retirement-error panic output belongs to the existing deliberate failure fixture and its explicit fail-closed assertion. Full-source validation is separate; all eight backend gaps remain open.
