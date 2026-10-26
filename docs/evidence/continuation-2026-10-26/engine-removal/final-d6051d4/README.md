# d6051d4 validation — native failed, Android passed

Native and Android input maps match exactly (341 source files). Full native validation failed at the recovery capacity test after format/Clippy passed; release and final traceability stages did not run. The failure and corrective follow-up are retained independently. This is not a complete native success.

`python3 tools/validate_android.py --gradle /tmp/stem-build-tools/gradle-8.9/bin/gradle` passed all nine stages, with 362 tests, zero failures/errors/skips and both desktop JNI smoke forms passing 40 groups under this revision's validator policy. Commands, environment, hashes, XML and lint are retained. The source predates JNI removal and the recovery-capacity test correction.

Android success does not replace the failed native run or establish device/model quality/licensing acceptance. Full corrected-source native validation remains necessary; all eight backend gaps remain open.
