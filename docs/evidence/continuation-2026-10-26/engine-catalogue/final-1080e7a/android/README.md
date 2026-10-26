# Catalogue native-source Android validation

Source `1080e7ae13e25fe78eea609a50e3280243b0040b`; 332 fingerprinted inputs. `python3 tools/validate_android.py --gradle /tmp/stem-build-tools/gradle-8.9/bin/gradle` with cached JDK21/SDK/Gradle environment in report: all nine stages passed, 359 tests with zero failures/errors/skips, both desktop JNI smoke forms passed 39 groups. This source predates JNI/Android catalogue adoption and read-only recovery. Native full validation of the same source remains running separately.

An initial launcher invocation omitted the Gradle path and exited at argument parsing before any checks; retained separately. No device, real-model, recognition quality or licence acceptance is implied. All eight backend gaps remain open.
