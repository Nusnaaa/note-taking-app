package org.stemnotebook.backend;
/** Trusted application boundary; never made available to model workers. */
public final class NativeBridge {
    static { System.loadLibrary("stem_bridge"); }
    private NativeBridge() {}
    public static native long open(String absoluteDatabasePath);
    /** Private opaque recovery binding; never grants content access. */
    public static native String captureLibraryIdentity(long handle);
    public static native String request(long handle, String request);
    /** Trusted observed-contact hint, no content or persistence acknowledgement. */
    public static native void setCapturePriority(long handle, String session, boolean active);
    public static native String recognitionStatus(long handle);
    public interface Verification { boolean verify(String challenge); }
    public static native void authenticate(long handle, String scopes, Verification verification);
    /** Trusted provider observation; never populated from JSON or supplied by model code. */
    public interface PdfGrant { boolean verify(String challenge, String target, String locator, String revision); }
    public static native String preparePdfSelection(long handle, String target, String locator, String revision, PdfGrant proof);
    public static native String finishPdfSelection(long handle, String review, boolean confirmed, PdfGrant proof);
    /** Worker-only review of the committed library; private capture journals are
     * outside this dataset. App transport must reconcile those before publication. */
    public static native String backupGeneration(long handle);
    public static native BackupReview reviewBackup(long handle, boolean originalInk, boolean sourceAssets);
    private static native void armBackup(long handle, long token, long budgetMillis);
    private static native char[] runBackup(long handle, long token, char[] password,
            Verification proof, boolean confirmed, BackupOutput output);
    private static native void releaseBackup(long handle, long token);

    public static final class BackupReview implements AutoCloseable {
        private final long host;
        private long token;
        public final boolean includesOriginalInk;
        public final long sourceAssets;
        private final String[] omissions;
        private BackupReview(long host, long token, boolean ink, long assets, String[] omissions) {
            this.host=host; this.token=token; includesOriginalInk=ink;
            sourceAssets=assets; this.omissions=omissions.clone();
        }
        public String[] omissions() { return omissions.clone(); }
        /** Engineering input bound, not a performance guarantee. Authority may expire sooner. */
        public synchronized BackupJob begin(long budgetMillis) {
            if (token==0 || budgetMillis<1 || budgetMillis>3600000) throw backupFailure();
            armBackup(host,token,budgetMillis);
            BackupJob job=new BackupJob(host,token); token=0; return job;
        }
        @Override public synchronized void close() {
            long old=token; token=0; if(old!=0) releaseBackup(host,old);
        }
    }
    /** Trusted sink receives ciphertext only. retire must close every owned resource
     * before returning, including when it reports failure; external resource clones
     * are outside this contract. Callback diagnostics are never propagated. */
    public interface BackupOutput {
        void write(byte[] ciphertext) throws Exception;
        void flush() throws Exception;
        void retire() throws Exception;
    }
    private static final class RetiringOutput implements BackupOutput {
        private final BackupOutput target;
        private boolean retired;
        private boolean failed;
        RetiringOutput(BackupOutput target) { this.target=target; }
        public void write(byte[] bytes) throws Exception { target.write(bytes); }
        public void flush() throws Exception { target.flush(); }
        public synchronized void retire() throws Exception {
            if(retired) { if(failed) throw backupFailure(); return; }
            retired=true;
            try { target.retire(); } catch(Throwable error) { failed=true; throw backupFailure(); }
        }
    }
    public static final class BackupJob implements AutoCloseable {
        private final long host;
        private final long token;
        private boolean started;
        private boolean closed;
        private BackupJob(long host,long token) { this.host=host; this.token=token; }
        /** Run on a dedicated optional worker, never the authoring executor.
         * Caller owns and must clear its password and returned recovery key. */
        public char[] run(char[] password, Verification proof, boolean confirmed, BackupOutput output) {
            if(output==null) throw backupFailure();
            RetiringOutput sink=new RetiringOutput(output);
            char[] snapshot=null;
            char[] key=null;
            try {
                synchronized(this) {
                    if(started || closed) throw backupFailure();
                    started=true;
                }
                if(password==null || password.length>1024 || proof==null) throw backupFailure();
                snapshot=password.clone();
                key=runBackup(host,token,snapshot,proof,confirmed,sink);
                sink.retire();
                if(key==null || key.length!=64) throw backupFailure();
                close();
                char[] result=key; key=null; return result;
            } catch(Throwable error) { throw backupFailure(); }
            finally {
                if(snapshot!=null) java.util.Arrays.fill(snapshot,'\0');
                if(key!=null) java.util.Arrays.fill(key,'\0');
                try { sink.retire(); } catch(Throwable error) { /* Native success is checked before key delivery. */ }
                close();
            }
        }
        @Override public void close() {
            synchronized(this) { if(closed) return; closed=true; }
            releaseBackup(host,token);
        }
    }
    private static IllegalStateException backupFailure() {
        return new IllegalStateException("Native backup operation failed");
    }
    public static native void close(long handle);
}
