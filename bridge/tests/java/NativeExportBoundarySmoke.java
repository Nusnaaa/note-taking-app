import org.stemnotebook.backend.NativeBridge;
import java.nio.file.Files;
import java.nio.charset.StandardCharsets;
import java.util.*;
import java.util.regex.Pattern;

/** Real desktop JNI native-copy delivery; no Android/provider/device claim. */
public final class NativeExportBoundarySmoke {
    static void check(boolean value) {if(!value)throw new AssertionError("native export boundary contract failed");}
    static void rejected(Runnable action) {try {action.run();throw new AssertionError("invalid export accepted");}catch(IllegalStateException error){check(error.getMessage().equals("Native export operation failed"));}}
    static String call(long host,String command) {String value=NativeBridge.request(host,"{\"version\":1,\"command\":"+command+"}");check(value.contains("\"ok\":true"));return value;}
    static String id(String json) {var m=Pattern.compile("\"id\":\"([^\"]+)\"").matcher(json);check(m.find());return m.group(1);}
    static long revision(long host,String id) {var m=Pattern.compile("\"revision\":([0-9]+)").matcher(call(host,"{\"op\":\"Read\",\"id\":\""+id+"\"}"));check(m.find());return Long.parseLong(m.group(1));}
    static String select(String note) {return "{\"Entities\":[\""+note+"\"]}";}
    public static void run() throws Exception {
        long host=NativeBridge.open(Files.createTempDirectory("stem-native-export").resolve("library.db").toString());
        try {
            String note=id(call(host,"{\"op\":\"Create\",\"kind\":\"Note\",\"parent\":null,\"title\":\"native copy λ\"}"));
            call(host,"{\"op\":\"Append\",\"entity\":\""+note+"\",\"content\":{\"type\":\"Text\",\"value\":\"001 + 1 = 3\"}}");
            String stroke=UUID.randomUUID().toString();
            call(host,"{\"op\":\"Capture\",\"ticket\":\""+UUID.randomUUID()+"\",\"entity\":\""+note+"\",\"strokes\":[{\"id\":\""+stroke+"\",\"capture_session\":\""+UUID.randomUUID()+"\",\"sequence\":0,\"pointer_id\":1,\"coordinate_space\":\"synthetic-jni-export\",\"tool\":\"Pen\",\"points\":[{\"x\":1.2345678901234567,\"y\":2.0,\"time_us\":1,\"pressure\":null,\"tilt_rad\":null,\"orientation_rad\":null,\"buttons\":null}]}]}");
            rejected(()->NativeBridge.reviewNativeExport(host,select(note),false));
            try(var review=NativeBridge.reviewNativeExport(host,select(note),true)) {
                check(review.summary.contains("\"includes_raw_ink\":true"));
                rejected(()->NativeBridge.reviewNativeExport(host,select(note),true));
                try(var data=review.finish(null,c->true,false,true,30000)) {
                    String encoded=new String(data.bytes,StandardCharsets.UTF_8);
                    check(encoded.contains("001 + 1 = 3") && encoded.contains(stroke) && encoded.contains("1.2345678901234567"));
                    check(data.recoveryKey==null);
                    data.close(); for(byte value:data.bytes)check(value==0);
                }
                rejected(()->review.finish(null,c->true,false,true,30000));
            }
            var stale=NativeBridge.reviewNativeExport(host,select(note),true);
            call(host,"{\"op\":\"Append\",\"entity\":\""+note+"\",\"content\":{\"type\":\"Text\",\"value\":\"later\"}}");
            rejected(()->stale.finish(null,c->true,false,true,30000));
            String pen=UUID.randomUUID().toString();
            var cancelled=NativeBridge.reviewNativeExport(host,select(note),true);
            NativeBridge.setCapturePriority(host,pen,true);
            rejected(()->cancelled.finish(null,c->true,false,true,30000));
            rejected(()->NativeBridge.reviewNativeExport(host,select(note),true));
            NativeBridge.setCapturePriority(host,pen,false);
            for(String invalid:new String[]{"{}","{\"Entities\":[]}","{\"Entities\":[\"bad\"]}","{\"Entities\":[\""+note+"\"],\"unknown\":true}"}) rejected(()->NativeBridge.reviewNativeExport(host,invalid,true));
            System.out.println("PASS: JNI native-copy original ink delivery, explicit source choice, single use, bounded review admission, stale/capture rejection and malformed selection (1)");
            NativeBridge.authenticate(host,"[{\"Entity\":\""+note+"\"}]",c->true);
            call(host,"{\"op\":\"SetLock\",\"entity\":\""+note+"\",\"revision\":"+revision(host,note)+",\"locked\":true}");
            var denied=NativeBridge.reviewNativeExport(host,select(note),true);
            rejected(()->denied.finish(null,c->false,true,true,30000));
            var unacknowledged=NativeBridge.reviewNativeExport(host,select(note),true);
            rejected(()->unacknowledged.finish(null,c->true,false,true,30000));
            var throwing=NativeBridge.reviewNativeExport(host,select(note),true);
            rejected(()->throwing.finish(null,c->{throw new IllegalArgumentException("private verifier diagnostic");},true,true,30000));
            var malformedPassword=NativeBridge.reviewNativeExport(host,select(note),true);
            rejected(()->malformedPassword.finish(new char[]{(char)0xd800},c->true,false,true,30000));
            char[] password="deliberate native-copy encryption passphrase".toCharArray();char[] original=password.clone();
            try(var review=NativeBridge.reviewNativeExport(host,select(note),true);var data=review.finish(password,c->{check(NativeBridge.captureLibraryIdentity(host).length()==36);return true;},false,true,120000)) {
                check(new String(data.bytes,0,8,StandardCharsets.US_ASCII).equals("STEMENC1"));check(data.recoveryKey.length==64);check(Arrays.equals(password,original));
                check(!new String(data.bytes,StandardCharsets.ISO_8859_1).contains("001 + 1 = 3"));
            } finally {Arrays.fill(password,'\0');Arrays.fill(original,'\0');}
            System.out.println("PASS: JNI native-copy fresh platform proof, off-writer callback, protected plaintext acknowledgment, encrypted bytes and recovery-key ownership (1)");
            var closing=NativeBridge.reviewNativeExport(host,select(note),true);
            rejected(()->closing.finish(null,c->{closing.close();return true;},true,true,30000));
            try(var review=NativeBridge.reviewNativeExport(host,select(note),true);var data=review.finish(null,c->true,true,true,30000)) {check(data.bytes.length>0);}
            var relocked=NativeBridge.reviewNativeExport(host,select(note),true);
            rejected(()->relocked.finish(null,c->{call(host,"{\"op\":\"Relock\"}");return true;},true,true,30000));
            System.out.println("PASS: JNI native-copy callback cancellation, admission retirement and relock refusal without output (1)");
        } finally {NativeBridge.close(host);}
    }
}
