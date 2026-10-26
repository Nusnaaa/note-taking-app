# ADR 019 — ready-runtime handoff for replacement

Status: recorded core engineering prerequisite; durable/host/JNI replacement-controller integration remains open.

A replacement must probe its candidate outside the existing Manager lock and preserve the current engine when that probe fails. A controller can build and activate an isolated candidate Manager using the owned probe APIs. `take_ready(id)` then extracts only a Ready, valid candidate into a private ReadyRuntime, clears its source defaults and invalidates work prepared through that source Manager. Abandoned candidates remain owned until the caller disposes of them off lock/worker.

`install_ready(&mut candidate)` publishes the exact probed adapter with a fresh destination validity epoch, preserving compatible task defaults and removing unsupported ones. Family mismatch or consumed candidate fails before mutation and leaves the candidate holder intact; the borrowed candidate avoids disposal under the destination lock on failure. Successful publication returns the old entry as DetachedRuntime for disposal after unlocking. Old work remains pinned but invalid; a repeated consumed candidate cannot reinstall itself.

These are controlled native primitives, not an owner-review or durable installation transaction. Controllers must revalidate their reviewed target/configuration under the destination lock and perform the existing consent/resource/durable-stage checks. This API alone does not fence a delayed UI review against independent later configuration changes. Model files/global cleanup acknowledgements, host-owned update capabilities, durable staged publication and JNI provider integration remain open. No model is installed or enabled by adding these Rust APIs.
