# JNI host admission boundary, 2026-10-25

This increment bounds Java UTF-16 database path input before requesting a native
string copy, retaining the existing UTF-8 byte and absolute-path checks. Host
handles use checked atomic allocation, stop at exhaustion and never wrap. Allocation
precedes SQLite opening so exhaustion cannot create a library. Failed opens may
consume a handle; handles are intentionally never reused.

The Rust regression fixture seeds an independent allocator at the integer boundary,
checks repeated exhausted allocation and rejects nonpositive allocator state.
`bridge/tests/java/OpenBoundarySmoke.java` exercises the actual desktop JVM/JNI boundary
with null, relative, overlong ASCII, multibyte and million-character paths and
checks valid-library close/reopen. It is synthetic desktop coverage, not Android
or device evidence. Commands/results are in `jni-open-smoke.log`; native suite
results are in the validation runner's adjacent logs and JSON.

Development attempts first failed rustfmt, then Clippy's test-module placement
rule. Formatting was applied, the test module moved after production items, and
the existing bounded-string helper reused. The validation runner was rerun after
these corrections. Previous Android/latest evidence in the working tree belongs
to earlier restore work and is not claimed as validation of this increment.

The owner requested periodic pushes and continued backend implementation on
2026-10-25. The pre-existing backend branch through 2d1407f was successfully pushed
to the configured origin, https://github.com/Nusnaaa/note-taking-app.git.
JNI/Android restore integration and the other recorded backend gaps remain open;
this fix does not open the backend acceptance gate.

The full runner completed with 310 tests passing, no failures/ignored tests, and
format/lint/release/traceability checks passing. It started before JNI restore was
added: its source fingerprint describes the initial boundary-fix snapshot; restore
work was added while the core tests ran and before the release build. Consequently
this is retained as development evidence, not one exact-source full-backend
acceptance run. The separate final JNI runner fingerprints and tests the restore
implementation, including newly added source inputs. Android was not rerun here.
