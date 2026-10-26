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

-keepclassmembers class * implements org.stemnotebook.backend.NativeBridge$RecogniserFactory {
    public org.stemnotebook.backend.NativeBridge$Recogniser create(java.lang.String, byte[], org.stemnotebook.backend.NativeBridge$EngineControl);
}
-keepclassmembers class * implements org.stemnotebook.backend.NativeBridge$Recogniser {
    public void probe(org.stemnotebook.backend.NativeBridge$EngineControl);
    public java.lang.String recognise(java.lang.String, java.lang.String, org.stemnotebook.backend.NativeBridge$EngineControl);
    public void retire();
}

-keepclassmembers class * implements org.stemnotebook.backend.NativeBridge$EngineInput {
    public int read(byte[], org.stemnotebook.backend.NativeBridge$EngineControl);
    public void retire();
}

-keepclassmembers class * implements org.stemnotebook.backend.NativeBridge$EngineInputFactory {
    public org.stemnotebook.backend.NativeBridge$EngineInput open(org.stemnotebook.backend.NativeBridge$EngineControl);
}

-keepclassmembers class * implements org.stemnotebook.backend.NativeBridge$ConverterFactory {
    public org.stemnotebook.backend.NativeBridge$Converter create(java.lang.String, byte[], org.stemnotebook.backend.NativeBridge$EngineControl);
}
-keepclassmembers class * implements org.stemnotebook.backend.NativeBridge$Converter {
    public void probe(org.stemnotebook.backend.NativeBridge$EngineControl);
    public java.lang.String formats(org.stemnotebook.backend.NativeBridge$EngineControl);
    public java.lang.String convert(java.lang.String, java.lang.String, org.stemnotebook.backend.NativeBridge$EngineControl);
    public void retire();
}
