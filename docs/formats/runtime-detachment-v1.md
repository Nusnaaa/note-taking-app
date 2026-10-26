# Runtime detachment API, version 1

Trusted native code can detach under its Manager lock and dispose after unlocking:

```rust
let detached = manager.detach(engine_id)?;
// release the Manager lock
// transfer detached to the optional retirement worker and drop it there
```

`DetachedRuntime::descriptor()` reports the exact removed binding. Private fields prevent reinsertion or fabricated completion. New work cannot select the removed entry; outstanding work stays pinned but its old epochs are invalid. `remove(id)` keeps its synchronous compatibility semantics.

For host-owned disposal, reserve `host.detachment_source()`, then call `source.detach(&mut manager, engine_id)` under the current app/capture authority. The private result exposes its ticket and removed descriptor. Release the Manager lock, then drop the holder; its payload retires off writer while admission and library ownership remain held through actual disposal.

This reports logical detachment and holder ownership. Other workers may still pin the runtime, and resource files remain guarded by their separate leases. It is not a successful global uninstall/resource-release receipt. No notebook JSON command or Java provider API is added here. See ADR 018 for remaining controller/global lifecycle work.
