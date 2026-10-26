Host-private runtime: complete native verification
==================================================

Exact 6159ff6 passed format, workspace all-target Clippy, all 404 native tests (zero failed/ignored), release build and traceability. Build/test debug were disabled; RUST_TEST_THREADS=2. Commands/results and source hashes are retained. No same-source full Android run is claimed; the later verified-factory Android source includes this manager but also changes other inputs. This predates JNI provider integration and does not establish actual device/SDK behaviour or explain retained E01/E02 watchdog failures. All eight backend gaps remain open.
