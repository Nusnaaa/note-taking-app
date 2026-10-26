# Host activation capability API, version 1

Trusted compiled backend code can:

1. Call `host.activation_source()` to reserve current app-authorised optional work. This sends metadata only to the writer.
2. On an optional worker, prepare with `source.prepare(&mut manager, id, reviewed_resource, consented)`. Release any Manager lock afterward. Oversized descriptors fail before invalidating a currently ready engine.
3. Run `work.run(&control)` on the optional worker. A current run returns a private `HostCompletedActivation`, including a provider failure outcome for explicit completion. Host cancellation retires the result without returning a usable capability.
4. Apply through the originating host with `host.complete_activation(&mut manager, &completed)`. Foreign hosts, stale app/capture epochs, superseded entries and replay cannot enable an engine.
5. Drop the private capability after releasing any Manager lock. Idle payload destruction is queued off writer; host close continues to own admitted work through actual retirement.

Source/work/result tickets identify ephemeral private jobs; notebook JSON cannot manufacture them. The holder includes no fabricated source ink or document identity. Preparation and running probes appear as running in the shared optional scope counters; idle/retiring work remains reserved until cleanup finishes. Ordinary authoring and its save acknowledgement remain independent.

See ADR 017 for scope. This is a native Rust API; Java provider registration/controller wiring and global resource lifecycle remain open.
