package org.stemnotebook.backend;
/** Trusted application boundary; never made available to model workers. */
public final class NativeBridge {
    static { System.loadLibrary("stem_bridge"); }
    private NativeBridge() {}
    /** Bundled generic guidance; no open library or notebook data is required. */
    public static native String help(String topic);
    public static native long open(String absoluteDatabasePath);
    /** Strict existing-library activation; no empty/missing/unknown initialization. */
    public static native long openExisting(String absoluteDatabasePath);
    /** Private opaque recovery binding; never grants content access. */
    public static native String captureLibraryIdentity(long handle);
    public static native String request(long handle, String request);
    /** Trusted observed-contact hint, no content or persistence acknowledgement. */
    public static native void setCapturePriority(long handle, String session, boolean active);
    public static native String recognitionStatus(long handle);
    /** Trusted app-compiled descriptor only, with explicit installation consent.
     * This stages no model file and does not establish runtime readiness. */
    /** Metadata snapshot; installed versions require explicit runtime activation. */
    public static native String engineCatalogue(long host, String after, int limit);
    public static native void selectEngineDefault(long host, String task, String engine);
    public static native String stageResourceFreeEngine(long handle, String descriptor, boolean confirmed);
    private static native long beginEngineActivation(long host, String version, long maxLoadBytes, long budgetMillis);
    private static native long beginEngineRecognition(long host, String block, String region, String task, String engine, long budgetMillis);
    private static native void runEngineActivation(long host, long token, RecogniserFactory factory);
    private static native String runEngineRecognition(long host, long token);
    private static native void releaseEngineJob(long host, long token);
    private static native boolean checkEngineControl(long token);
    /** Valid only during its callback. Retaining it cannot extend authority. */
    public static final class EngineControl {
        private final long token;
        private EngineControl(long token) { this.token=token; }
        public boolean active() { return checkEngineControl(token); }
        public void check() { if(!active()) throw engineFailure(); }
    }
    /** SDK construction runs on the optional caller worker, after integrity and
     * consent checks. Resource bytes are wiped after create returns. Load them or
     * keep a protected copy before returning; null means no staged resource.
     * A throwing factory must retire every partially constructed SDK itself.
     * No network, telemetry or raster requirement is permitted. */
    public interface RecogniserFactory {
        Recogniser create(String descriptor, byte[] verifiedResource, EngineControl control) throws Exception;
    }
    /** Trusted local ordered digital-ink adapter. Output is RecognitionOutput
     * JSON; core validates modality, bounds and freshness before storing review.
     * retire must release every owned SDK resource before returning, including
     * when it throws. Other SDK clones are outside this cooperative contract. */
    public interface Recogniser {
        void probe(EngineControl control) throws Exception;
        String recognise(String orderedInk, String task, EngineControl control) throws Exception;
        void retire() throws Exception;
    }
    private static IllegalStateException engineFailure() { return new IllegalStateException("Native notebook operation failed"); }
    /** All begin/run calls belong on an optional worker, never the authoring
     * executor. Close cancels; it does not acknowledge global SDK retirement. */
    public static final class EngineJob implements AutoCloseable {
        private final long host, token;
        private final boolean activation;
        private boolean started, closed;
        private EngineJob(long host, long token, boolean activation) { this.host=host; this.token=token; this.activation=activation; }
        private synchronized void start(boolean expected) { if(started || closed || expected!=activation) throw engineFailure(); started=true; }
        public void activate(RecogniserFactory factory) {
            start(true);
            try { if(factory==null) throw engineFailure(); runEngineActivation(host,token,factory); }
            finally { close(); }
        }
        /** Returns a durable pending proposal; source ink is unchanged. */
        public String recognise() {
            start(false);
            try { return runEngineRecognition(host,token); }
            finally { close(); }
        }
        @Override public void close() {
            synchronized(this) { if(closed) return; closed=true; }
            releaseEngineJob(host,token);
        }
    }
    public static EngineJob engineActivation(long host, String version, long maxLoadBytes, long budgetMillis) {
        long token=beginEngineActivation(host,version,maxLoadBytes,budgetMillis);
        try { return new EngineJob(host,token,true); }
        catch(Throwable failure) { releaseEngineJob(host,token); throw failure; }
    }
    public static EngineJob engineRecognition(long host, String block, String region, String task, String engine, long budgetMillis) {
        long token=beginEngineRecognition(host,block,region,task,engine,budgetMillis);
        try { return new EngineJob(host,token,false); }
        catch(Throwable failure) { releaseEngineJob(host,token); throw failure; }
    }
    private static native long beginEngineRecovery(long host, long budgetMillis);
    private static native String runEngineRecovery(long host, long token, String privateRoot, String after, int limit);
    /** Single-use read-only scan, cancelled through the same native scope as imports. */
    public static final class EngineRecovery implements AutoCloseable {
        private final long host, token;
        private boolean started, closed;
        private EngineRecovery(long host,long token) { this.host=host;this.token=token; }
        public String scan(String privateRoot,String after,int limit) {
            synchronized(this) { if(started || closed) throw engineFailure(); started=true; }
            try { return runEngineRecovery(host,token,privateRoot,after,limit); }
            finally { close(); }
        }
        @Override public void close() {
            synchronized(this) { if(closed) return;closed=true; }
            releaseEngineJob(host,token);
        }
    }
    public static EngineRecovery engineRecovery(long host,long budgetMillis) {
        long token=beginEngineRecovery(host,budgetMillis);
        try { return new EngineRecovery(host,token); }
        catch(Throwable error) { releaseEngineJob(host,token);throw error; }
    }
    private static native long beginEngineImport(long host, String descriptor, long total, boolean confirmed, long budgetMillis);
    private static native EngineImport startEngineImport(long host, long token, String privateRoot, String resume);
    private static native String copyEngineImport(long host, long token, EngineInputFactory input);
    /** Selected local model input positioned at committedBytes. read must return
     * -1 for EOF or 1..buffer.length; zero/oversized counts fail. Retained buffers
     * are cleared. retire must close all owned input handles even when throwing. */
    /** Open only inside native running admission. A throwing factory must retire
     * all partial resources itself; the control is valid during open only. */
    public interface EngineInputFactory { EngineInput open(EngineControl control) throws Exception; }
    public interface EngineInput {
        int read(byte[] buffer, EngineControl control) throws Exception;
        void retire() throws Exception;
    }
    private static final class RetiringEngineInput implements EngineInput {
        private final EngineInput target;
        private boolean retired, failed;
        RetiringEngineInput(EngineInput target) { this.target=target; }
        public int read(byte[] buffer, EngineControl control) throws Exception { return target.read(buffer,control); }
        public synchronized void retire() throws Exception {
            if(retired) { if(failed) throw engineFailure(); return; }
            retired=true;
            try { target.retire(); } catch(Throwable error) { failed=true; throw engineFailure(); }
        }
    }
    /** Durable identity/offset available before opening the selected input.
     * Failure/cancel preserves private transfer files; close is not disk discard. */
    public static final class EngineImport implements AutoCloseable {
        private final long host, token;
        public final String version;
        public final long committedBytes;
        private boolean started, closed;
        private EngineImport(long host,long token,String version,long committed) {
            this.host=host;this.token=token;this.version=version;committedBytes=committed;
        }
        public String copy(EngineInputFactory input) {
            if(input==null) throw engineFailure();
            synchronized(this) { if(started || closed) throw engineFailure(); started=true; }
            try {
                return copyEngineImport(host,token,control -> {
                    EngineInput opened=input.open(control);
                    if(opened==null) throw engineFailure();
                    try { return new RetiringEngineInput(opened); }
                    catch(Throwable error) { try {opened.retire();} catch(Throwable ignored) {} throw engineFailure(); }
                });
            } catch(Throwable error) { throw engineFailure(); }
            finally {close();}
        }
        @Override public void close() {
            synchronized(this) {if(closed) return;closed=true;}
            releaseEngineJob(host,token);
        }
    }
    /** Optional-worker operation. privateRoot must be app-owned and excluded
     * from OS cloud backup. User-selected bytes/consent never come from JSON. */
    public static EngineImport engineImport(long host,String descriptor,long total,boolean confirmed,String privateRoot,String resume,long budgetMillis) {
        long token=beginEngineImport(host,descriptor,total,confirmed,budgetMillis);
        try {return startEngineImport(host,token,privateRoot,resume);}
        catch(Throwable error) {releaseEngineJob(host,token);throw engineFailure();}
    }
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
    /** Trusted policy/Keystore verifier for encrypted backup only; never model JSON. */
    public interface BackupAuthorization {
        boolean verify(String challenge, int version, String library, String scopeDigest, boolean protectedContent);
    }
    public static native BackupReview reviewBackupDelegated(long handle, boolean originalInk, boolean sourceAssets, BackupAuthorization proof);
    /** Compare a previously authorized artifact stamp; never exposes row data. */
    public static native boolean backupSnapshotMatches(long handle, String library, String generation);
    private static native void armBackup(long handle, long token, long budgetMillis);
    private static native char[] runBackup(long handle, long token, char[] password,
            Verification proof, boolean confirmed, BackupOutput output);
    private static native void releaseBackup(long handle, long token);

    public static final class BackupAuthority {
        public final int version;
        public final String library, scopeDigest;
        public final boolean protectedContent;
        private BackupAuthority(int version, String library, String digest, boolean protectedContent) {
            if(version!=1 || library==null || library.length()!=36 || digest==null || !digest.matches("[0-9a-f]{64}")) throw backupFailure();
            this.version=version; this.library=library; scopeDigest=digest; this.protectedContent=protectedContent;
        }
    }
    public static final class BackupReview implements AutoCloseable {
        private final long host;
        private long token;
        public final boolean includesOriginalInk;
        public final long sourceAssets;
        private final String[] omissions;
        public final BackupAuthority authority;
        public final String generation;
        private BackupReview(long host, long token, boolean ink, long assets, String[] omissions, int version, String library, String digest, String generation, boolean protectedContent) {
            this.host=host; this.token=token; includesOriginalInk=ink;
            sourceAssets=assets; this.omissions=omissions.clone();
            authority=new BackupAuthority(version,library,digest,protectedContent); this.generation=generation;
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
    private static native long armRestore(long host, long budgetMillis);
    private static native String prepareRestore(long host, long token, char[] credential,
            boolean recoveryKey, RestoreInput input, String privateDirectory, long maxPlaintextBytes);
    private static native String publishRestore(long host, long token, String destination,
            Verification proof, boolean confirmed);
    private static native void releaseRestore(long host, long token);
    /** Ciphertext only. read returns -1 at EOF, otherwise 1..buffer.length.
     * retire must close all owned input resources, even when it reports failure. */
    public interface RestoreInput {
        int read(byte[] ciphertext) throws Exception;
        void retire() throws Exception;
    }
    private static final class RetiringInput implements RestoreInput {
        private final RestoreInput target;
        private boolean retired;
        private boolean failed;
        RetiringInput(RestoreInput target) { this.target=target; }
        public int read(byte[] bytes) throws Exception { return target.read(bytes); }
        public synchronized void retire() throws Exception {
            if(retired) { if(failed) throw restoreFailure(); return; }
            retired=true;
            try { target.retire(); } catch(Throwable error) { failed=true; throw restoreFailure(); }
        }
    }
    /** Trusted optional worker capability. Paths must be app-private. The app must
     * own pre/post-native work through full shutdown, independently of this job. */
    public static RestoreJob beginRestore(long host, long budgetMillis) {
        if(budgetMillis<1 || budgetMillis>3600000) throw restoreFailure();
        return new RestoreJob(host, armRestore(host,budgetMillis));
    }
    public static final class RestoreJob implements AutoCloseable {
        private final long host;
        private final long token;
        private boolean started;
        private boolean prepared;
        private boolean publishing;
        private boolean closed;
        private RestoreJob(long host,long token) { this.host=host; this.token=token; }
        /** Caller owns and must clear its credential. Summary contains counts only;
         * no plaintext content, stage path or credential is returned. */
        public String prepare(char[] credential, boolean recoveryKey, RestoreInput input,
                String privateDirectory, long maxPlaintextBytes) {
            if(input==null) throw restoreFailure();
            RetiringInput source=new RetiringInput(input);
            char[] snapshot=null;
            boolean success=false;
            try {
                synchronized(this) {
                    if(started || closed) throw restoreFailure();
                    started=true;
                }
                if(credential==null || credential.length>1024 || maxPlaintextBytes<1) throw restoreFailure();
                snapshot=credential.clone();
                String summary=prepareRestore(host,token,snapshot,recoveryKey,source,privateDirectory,maxPlaintextBytes);
                source.retire();
                if(summary==null) throw restoreFailure();
                synchronized(this) {
                    if(closed) throw restoreFailure();
                    prepared=true;
                }
                success=true;
                return summary;
            } catch(Throwable error) { throw restoreFailure(); }
            finally {
                if(snapshot!=null) java.util.Arrays.fill(snapshot,'\0');
                try { source.retire(); } catch(Throwable error) { /* Failure already suppresses success above. */ }
                if(!success) close();
            }
        }
        /** Fresh proof and explicit confirmation publish a separate, absent path.
         * Returns JSON "Published" or "Uncertain"; never activates the copy. */
        public String publish(String destination, Verification proof, boolean confirmed) {
            try {
                synchronized(this) {
                    if(!prepared || publishing || closed) throw restoreFailure();
                    publishing=true;
                }
                if(proof==null) throw restoreFailure();
                String result=publishRestore(host,token,destination,proof,confirmed);
                if(result==null) throw restoreFailure();
                return result;
            } catch(Throwable error) { throw restoreFailure(); }
            finally { close(); }
        }
        @Override public void close() {
            synchronized(this) { if(closed) return; closed=true; }
            releaseRestore(host,token);
        }
    }
    private static IllegalStateException restoreFailure() {
        return new IllegalStateException("Native restore operation failed");
    }
    private static IllegalStateException backupFailure() {
        return new IllegalStateException("Native backup operation failed");
    }
    public static native void close(long handle);
}
