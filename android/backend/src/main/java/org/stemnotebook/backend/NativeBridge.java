package org.stemnotebook.backend;
/** Trusted application boundary; never made available to model workers. */
public final class NativeBridge {
    static { System.loadLibrary("stem_bridge"); }
    private NativeBridge() {}
    public static native long open(String absoluteDatabasePath);
    public static native String request(long handle, String request);
    public interface Verification { boolean verify(String challenge); }
    public static native void authenticate(long handle, String scopes, Verification verification);
    public static native void close(long handle);
}
