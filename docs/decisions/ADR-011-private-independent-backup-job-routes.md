# ADR-011 — private independent-library backup job routing

Status: engineering decision under the approved optional native-backup requirement.

Use a private, FULL-synchronous SQLite route map under the primary application's
no-backup root. Persist root filesystem identity, canonical path, unique native
library UUID and exact schedule/revision before OS registration. A route is
metadata and does not enable native policy or authorize content. Existing
Arming/Enabled/Disabled policy, encrypted exact-scope delegation, destination
identity and live permission checks remain authoritative.

Allocate a finite reserved 32-job identifier namespace (primary slot plus
independent slots) with non-evicting transactions and unique root/library keys.
This is a defensive resource bound, not an SLO or a claim about OS availability.
Skip foreign occupied identifiers; never overwrite/cancel a foreign token. CAS
revision updates and strict known-schema validation preserve unknown/future files.
Serialize initialization across controllers; SQLite retains transaction ownership.
No job, route or credential is created merely by opening a notebook.

Keep only version and schedule/revision UUIDs in OS extras. Resolve root paths on
the JobService worker from the private map, check device/inode/UID and native
library identity, and acquire the existing-only shared runtime. Canonical aliases
share ownership. Another filesystem path with an already registered logical
library UUID requires a real independent import/restore identity rather than
silently taking over the original route. Legacy primary jobs alone may use the
primary fallback; unmapped independent jobs cannot open the primary notebook.

Serialize admitted library jobs in a bounded service queue. Match stop by job ID
and token, retain reservations/leases until actual work retirement, and skip root
resolution for stopped queued work. Unknown/replaced roots, credential failures
and lost OS acknowledgements remain unavailable/uncertain with existing receipts;
no permission, key, policy enablement or path repair is guessed.

The map contains no content or credentials, is not part of portable notebook
backups and is excluded from OS content backup/transfer. This does not claim
hostile privileged-filesystem resistance or actual device/Keystore/provider/boot
acceptance. Host tests establish component behavior only; the complete backend
and production H.I. gate remain open.
