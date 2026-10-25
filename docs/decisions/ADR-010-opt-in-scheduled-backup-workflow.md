# ADR 010 — opt-in scheduled backup workflow

25 October 2026. Backend implementation in progress.

Use the existing native encrypted backup and Android publication controllers for
scheduled work. Keep destination persistence, device credential storage and job
enablement separate reviewed decisions. Validate the password through an initial
native backup, offer its independent key before acceptance, and preserve known
publication/grant effects if later setup fails. Never invent an approving
background verifier or ordinary UI access grant.

Persist an explicit size limit and reviewed destination identity alongside the
configuration revision. Migrate only the exact known private policy v1 schema to
v2, preserving credentials, consent and runs while disabling old configurations
that lack those choices. Unknown/future files remain preserved and unavailable.
Known-file test mutations use NO_LOCALIZED_COLLATORS, matching production; default
Android SQLite flags can add android_metadata and otherwise test schema rejection
instead of the intended data/migration fault.

Use platform JobScheduler period limits and persisted UUID/revision-only jobs,
with BIND_JOB_SERVICE and explicit opt-in before registration. Scope the current
platform route to the primary app root; refuse unroutable independent roots until
a real dispatcher is implemented. Keep protected-content scheduling authority
and independent-root dispatch as required unfinished work, not completion by
restriction or silent omission. No account, cloud sync or OS content backup.

Hold shared runtime reservations/leases through actual retirement, including
canceled opening and constructor-failure cleanup. Match stop by token across
parameter instances. Retain source/key/provider work until real worker and
cancellation fences retire; OS stop/future cancellation is only an observation.
Disable durable policy/revision before canceling jobs and retiring the password.
Expose separate acknowledgements and retain uncertain state for owner review.

Use application publication UUIDs for per-backup encrypted recovery references.
Give subsequent copies of an artifact fresh catalogue IDs to preserve existing
multi-destination publication. Fresh verification guards key reveal/delete; no
password reveal and no plaintext/software-key fallback. Device retention does not
substitute for the independently stored portable credential path.

This preserves Rust portable-core/SQLite ownership and backend-before-H.I.
sequencing. See [the workflow contract](../formats/SCHEDULED_BACKUPS.md). Actual
Keystore/device/provider/physical interruption and complete backend acceptance
are not established by host workflow tests.
