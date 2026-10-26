# Native Host unpublished-import discard

ADR 031 implements stable action IDs; bounded app-only action history; worker-only preparation/deletion; linked capture/relock/caller control; metadata-only writer authorisation/completion; and phase-pinned guard ownership through actual retirement. Known SQL success is retained through late cancellation; uncertain physical/metadata outcomes keep explicit Pending retry.

Final component commands passed: 22 integration cases (Host discard 6, Core discard 9, existing Host import 7), two private writer-borrow ownership cases, all-target Clippy with denied warnings, formatting and traceability. Source hashes include new files; this is not a full clean-revision native/Android validator report. Full verification follows separately.

The retained lint failure led to boxing the metadata permit. The app-lock fixture failed because enabling app protection requires fresh authentication; the fixture now authenticates with an explicit synthetic trusted adapter, enables protection, then relocks before asserting denied history/plan access. No production authentication behavior was weakened.

JNI/Android discard, installed SDK file reclamation, actual recogniser/device checks and broader backend acceptance remain OPEN. The separately archived c386d15 native 466/Android 364/JNI 43 proof precedes this Host increment.
