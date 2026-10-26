# Stored engine activation plan, version 1

Trusted native code can:

1. Capture `store.engine_activation_plan(version_id, &access)`; this is metadata only.
2. On an optional worker outside the live Manager lock, call the plan's `prepare_recogniser` or `prepare_converter` with a compiled adapter, explicit load cap and JobControl.
3. Recheck current controller authority, then call `store.commit_engine_activation(&mut prepared, &mut manager, &access)` in the serialized metadata publication context.
4. Release locks and dispose any returned DetachedRuntime on the optional retirement worker.

The prepared holder retains its candidate on failed publication. A successful call consumes its plan; `version()` returns None and replay fails. Failed probe, resource hash/load cap, cancellation, stale generation, foreign library, current app access or SQL rollback preserves the previous active/runtime state. Ordinary notebook JSON cannot construct a plan or prepared SDK holder.

No schema changes occur. The private plan records existing staged-consent provenance and original access expiry; preparation checks the supplied control before/during reads and around probe execution. Host control/admission/retirement linkage and Java provider integration remain open. See ADR 020 for scope and existing synchronous compatibility behaviour.
