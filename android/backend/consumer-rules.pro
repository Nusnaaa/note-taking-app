# JNI uses exported names, the private BackupReview constructor and callback names.
# Keep this trusted boundary, rather than disabling shrinking for the backend.
-keep class org.stemnotebook.backend.NativeBridge { *; }
-keep class org.stemnotebook.backend.NativeBridge$* { *; }

# Concrete platform/lambda callback methods are invoked only from native code.
-keepclassmembers class * implements org.stemnotebook.backend.NativeBridge$Verification {
    public boolean verify(java.lang.String);
}
-keepclassmembers class * implements org.stemnotebook.backend.NativeBridge$PdfGrant {
    public boolean verify(java.lang.String, java.lang.String, java.lang.String, java.lang.String);
}
-keepclassmembers class * implements org.stemnotebook.backend.NativeBridge$BackupOutput {
    public void write(byte[]);
    public void flush();
    public void retire();
}

-keepclassmembers class * implements org.stemnotebook.backend.NativeBridge$RestoreInput {
    public int read(byte[]);
    public void retire();
}

-keepclassmembers class * implements org.stemnotebook.backend.NativeBridge$BackupAuthorization {
    public boolean verify(java.lang.String, int, java.lang.String, java.lang.String, boolean);
}
