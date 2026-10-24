Development-only dirty-source checks, not frozen-source acceptance:

- Initial compilation passed; twelve new provider fixtures plus the existing thirteen controller fixtures and lint passed in 3m45s. Actual JNI and host-backed descriptors are used with synthetic provider adapters.
- Adding a ParcelFileDescriptor.createPipe fixture transferred/verified ciphertext, but failed its assumption that descriptor sync must be unsupported: Robolectric acknowledged sync. Raw failure output/XML are retained. This does not establish actual ART/kernel-pipe semantics, nor was the controller claiming cloud durability.
- The fixture now explicitly injects SyncFailedException at descriptor sync and retains its false-acknowledgement assertion. The thirteen provider fixtures and lint then passed in 3m5s, with no weakened assertions/timeouts. The injected unsupported condition and simulated pipe are stated explicitly in source/evidence.
- Unknown provider diagnostics remain sanitized. E01 remains open; no underlying cause is invented.

Complete frozen-source validation separately covers all Android fixtures, Rust, optimized JNI and both AAR contracts. No host/synthetic-provider result is substituted for device, Binder, real-provider or cloud-durability evidence.
