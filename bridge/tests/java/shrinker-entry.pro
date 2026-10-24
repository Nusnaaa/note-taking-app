# Only the fixture entry point is kept. JNI names and callbacks depend on the
# actual AAR consumer rules, not broad test-package keeps or disabled shrinking.
-keep class BoundarySmoke { public static void main(java.lang.String[]); }
