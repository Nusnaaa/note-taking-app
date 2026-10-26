# Ready-runtime handoff API, version 1

Prepare and probe a candidate in an isolated Manager on an optional worker, with the existing explicit consent/resource checks. Then:

1. `candidate.take_ready(id)` removes a valid Ready entry into a private holder and invalidates source-prepared work.
2. Under the destination controller's current configuration/Manager lock, `destination.install_ready(&mut ready)` validates family compatibility and publishes a fresh runtime epoch.
3. Release the lock, then dispose of any returned old DetachedRuntime on the optional retirement worker.

Failure leaves the ready holder owned by the caller for off-lock disposal. Success consumes its private entry; `descriptor()` subsequently returns None and replay fails. A compatible selected task keeps its engine ID through the version change; unsupported tasks lose that default and get no invented fallback. A probe failure/unready candidate never replaces the existing engine.

This is a native in-memory handoff. Durable/host-controlled publication and reviewed configuration fencing remain separate, as described in ADR 019. Outstanding old workers continue to own their resources until actual retirement.
