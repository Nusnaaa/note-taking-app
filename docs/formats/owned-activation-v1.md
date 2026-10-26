# Owned activation API, version 1

Trusted compiled backend/provider code can use:

1. Under its Manager lock, call `prepare_activation(id, reviewed_resource_bytes, consented)`; retain the returned work and release the lock.
2. Run `work.run(&job_control)` on the optional worker. The result is an opaque `CompletedActivation` that owns the exact callback/resource holder and its private outcome.
3. Reacquire the Manager lock and call `complete_activation(&mut completed)`.
4. Release the lock, then drop the completion on the optional retirement worker. Keep actual ownership until callback/destruction finishes.

Dropping prepared or completed work without applying it never marks the entry Ready. A completion can apply its result once to the exact current entry/activation epoch. Consent/integrity, caller cancellation/expiry, supersession and shutdown retain their separate checks. Removal can invalidate work without waiting for an uncooperative probe; the work still pins resources through actual retirement.

No serialisable capability, JNI provider API, automatic model installation or notebook schema change is introduced by this API. See ADR 016 for scope and limits.

The completion holder retains provider-owned error payloads as well as the adapter. The owned API returns a content-free error classification; drop the holder after unlocking even when completion fails or is cancelled. The synchronous compatibility wrapper retains its original provider-error result semantics.
