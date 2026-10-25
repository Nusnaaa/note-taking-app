# Trusted JNI recovered-copy transport — 25 October 2026

The JNI restore transport implements one host-bound, non-reused job token with
bounded cooperative deadline, owned ciphertext input/retirement, bounded
zero-cleared password or recovery-key copies, count-only stage review, and fresh
confirmed no-overwrite publication of a separate recovered library. It preserves
Published versus Uncertain and never activates the recovered library. Foreign
release cannot consume a token. Registry locks do not span callbacks, SQLite
stage retirement or host close. Running cancellation keeps admission until the
actual reader/stage work retires; idle close drops stages outside registry locks.

The Java wrapper retires caller inputs exactly once even after pre-native
validation failure and clears its credential snapshot. Native failure, malformed
UTF-16, invalid callback return values, pending callback exceptions and failed
retirement return generic diagnostics. Native callbacks receive ciphertext only.
The caller still owns and must clear the original credential array. Retirement
requires the trusted input implementation to close all its resources, including
when reporting failure; external clones are outside this contract.

`tools/validate_jni.py` runs format, bridge Clippy/unit tests/build, actual JVM/JNI
fixtures, optimized R8 classfile fixtures and traceability without pretending to
run ARM64, Gradle, provider or device checks. Unlike older validation tools, its
source fingerprint includes new untracked source inputs. It requires a local R8
JAR and never silently skips a missing tool. The full Android runner also includes
the new fixtures and concrete callback consumer rules for its next execution.

Run from the repository with Cargo and a JDK on PATH:

```
python3 tools/validate_jni.py --r8 /path/to/build-tools/35.0.0/lib/d8.jar --output docs/evidence/continuation-2026-10-25/jni-restore
```

Actual commands/results, source hashes and dirty-source status are retained in
`jni-validation.json` and adjacent logs. The new cases exercise password/key
restore, publication/reopen, declined confirmation, foreign-token isolation,
malformed secrets, invalid/zero/oversized reads, failed retirement, pre-admission
retirement, blocked cancellation and host-close stage cleanup. The allocator
boundary regression and actual JNI oversized-path checks are included too.
The first development Java compile rejected capture of a reassigned host local;
the fixture now keeps the host final with an independent closed flag. Earlier
JNI runner output is retained in `development/`; recovery-key/decline/malformed
credential cases were added before the final rerun.

The earlier full native run in `../jni-handles/` completed 310 tests successfully
but spans development changes and is explicitly not exact-source acceptance of
this final tree. No Android/device test result is claimed here. The pre-existing
`latest/` modifications and 24 October restore evidence were preserved separately.

Backend acceptance stays closed. Android input-grant and outer-worker ownership,
recovered-root selection/activation, quotas/orphan cleanup, full requirement
coverage and other documented backend gaps remain implementation work.
