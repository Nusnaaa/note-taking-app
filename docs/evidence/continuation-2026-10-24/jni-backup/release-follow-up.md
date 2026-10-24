Release follow-up identified during frozen-source validation of bc7d27c:

The Android backend library currently has no consumer ProGuard/R8 rules. JNI names the NativeBridge class, nested BackupReview constructor, Verification.verify and BackupOutput write/flush/retire callbacks. The current debug JVM/Robolectric/ARM64 AAR checks do not exercise shrinking. Before any release app, add packaged consumer keep rules covering JNI-reflected classes/members and validate the actual shrunk boundary (prefer the existing JVM smoke fixture under R8, plus AAR rule retention). This remains part of the open Android boundary/packaging gap and does not authorise production H.I.

The Android validator's legacy jni_assertions field remains 10; the expanded jni-smoke.log separately reports seven new backup groups and the existing ten groups. Update the reporting field to describe smoke groups accurately with the packaging increment; do not infer individual assertion counts from these labels.

Next app backup work remains a dedicated optional-worker controller, fresh capture/recovery-draft barrier, trusted encrypted file quota/sync/publication, and ownership through all pre-admission/native/post-native work. NotebookBackend.closeFullyAsync must await that controller before LibraryOwnership.close. A native close alone does not own a Java sink supplied before core admission.
