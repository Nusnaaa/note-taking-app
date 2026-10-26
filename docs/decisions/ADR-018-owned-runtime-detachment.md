# ADR 018 — owned runtime detachment and host retirement

Status: recorded backend engineering decision; durable model controllers/JNI/global resource integration remains open.

A manager lock must not invoke an idle provider's final destructor. Add `Manager::detach(id)`: remove the current entry and its defaults, invalidate inference/activation epochs, then return a private `DetachedRuntime` that still owns the exact descriptor/adapter. The caller releases its Manager lock before disposing of the holder on an optional worker. `Manager::remove` remains the synchronous compatibility wrapper.

Detachment is logical removal, not an acknowledgement that every old worker/resource retired. Outstanding recognition, conversion and activation work retain their independent pins and cannot publish a valid late result. Resource-file deletion continues to obey ResourceLeases; a detached-holder disposal does not bypass a still-running reader. Re-registering the same ID cannot accept an old activation token or silently restore a removed default.

For native host ownership, `Host::detachment_source` sends only app-authorised metadata to the writer. Its typed source checks current access/capture authority, descriptor-frame/aggregate admission and source phase before removing the entry under the scope state fence. The returned `HostDetachedRuntime` reports the descriptor actually detached. Actual adapter ownership stays in the private phase; dropping or invalidating the holder queues disposal on the optional retirement lane. Shutdown retains library ownership until that disposal finishes. Release Manager locks before waiting for host shutdown or cleanup.

The detachment source shares metadata admission and retirement startup checks with activation. No provider objects/callbacks enter these mailbox messages. Oversized/expired/cancelled reservations preserve the current engine. Completed logical removal stays removed even if its private holder is later cancelled; the holder does not reinstall a runtime.

No model file is deleted, installed or published by this API. Global resource/uninstall receipts, durable installation/removal controllers, staged replacement disposal, JNI providers, actual eligible recognisers and hard worker isolation remain open. Existing workers can keep a registered runtime's resources alive after this host's detached holder retires; other active scopes and file leases remain authoritative for their ownership.
