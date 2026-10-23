package org.stemnotebook.backend;
/** Trusted application boundary; never made available to model workers. */
public final class NativeBridge {
    static { System.loadLibrary("stem_bridge"); }
    private NativeBridge() {}
    public static native long open(String absoluteDatabasePath);
    /** Private opaque recovery binding; never grants content access. */
    public static native String captureLibraryIdentity(long handle);
    public static native String request(long handle, String request);
    public interface Verification { boolean verify(String challenge); }
    public static native void authenticate(long handle, String scopes, Verification verification);
    /** Trusted provider observation; never populated from JSON or supplied by model code. */
    public interface PdfGrant { boolean verify(String challenge, String target, String locator, String revision); }
    public static native String preparePdfSelection(long handle, String target, String locator, String revision, PdfGrant proof);
    public static native String finishPdfSelection(long handle, String review, boolean confirmed, PdfGrant proof);
    public static native void close(long handle);
}
