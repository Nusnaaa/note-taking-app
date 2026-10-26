# ADR 020 — stored engine activation plans

Status: recorded core backend decision; host worker/controller/JNI adoption remains open.

Separate stored-engine activation into metadata capture, optional-worker preparation and durable publication. `Store::engine_activation_plan` captures the library identity, exact engine version, store generation and original access expiry without file I/O or a provider callback. The private plan prepares a recogniser or converter on an optional worker without borrowing the live Manager: load bounded local resource bytes, verify the recorded hash, probe a candidate with JobControl and extract the exact ready runtime. Resource buffers are zero-cleared.

`Store::commit_engine_activation` rechecks current app access, the original expiry/control, library/version identity and generation. A borrowed ReadyPublication guard preflights family compatibility and holds exclusive destination/candidate references across the callback-free SQLite transaction. Failed metadata/transaction checks leave the candidate holder and old live runtime intact. After the durable commit, publication is infallible under those exclusive borrows and returns the old DetachedRuntime for disposal after unlocking. Repeated consumed plans fail.

The transaction records the installed version and active pointer using existing tables; no new schema or inferred runtime readiness is introduced. Probe failure never replaces the old runtime. Another library or authored/configuration change invalidates the prepared generation. Separate later resource-file changes are not a new model-availability guarantee; current runtime lifetime and future reactivation still obey resource guards/verification.

This is a native Store API. Call preparation on an optional worker and dispose failed/old SDK holders after releasing locks. Existing synchronous activation APIs remain available under their prior caller-owned rules. Host-owned plan execution/admission, actual provider/controller publication, global scheduling/uninstall and hard allocation/worker isolation remain open. Installed metadata is still distinct from fresh runtime readiness, and adding these APIs installs no real model.
