# ADR 009 — shared live backend ownership

25 October 2026. Backend implementation decision.

Foreground and scheduled work use distinct leases to a canonical-root
NotebookBackend through NotebookRuntime. Existing-only opening is the default;
only deliberate foreground first-use may create a library. Direct constructors
remain exclusive and are not adopted or displaced. Runtime possession gives no
protected-content authority.

Keep pending reservations and claimed leases separate; cancellation abandons
only its own reservation. Use private observation signals rather than exposing
continuations that could execute on native/registry/open workers. Hold the final
slot through actual library ownership release. A generic close failure cannot
prove release, so NotebookBackend now emits a separate success-only witness after
LibraryOwnership.close. Keep retirement denied on earlier cleanup failure.

Use separate bounded open and retirement workers: slow opens must not consume
the workers required to retire another root. Bound root slots and reservations,
including roots that are still opening/retiring. Release caller leases only after
that caller's optional/native/provider operations actually retire.

This is backend lifecycle support, not an enabled scheduler, device performance
claim or production H.I. Future job services must use this entry point and retain
their lease through actual stop/retirement. See the
[runtime contract](../formats/SHARED_NOTEBOOK_RUNTIME.md).
