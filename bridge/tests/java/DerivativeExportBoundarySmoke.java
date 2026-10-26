import org.stemnotebook.backend.NativeBridge;
import java.nio.charset.StandardCharsets;
import java.nio.file.*;
import java.util.*;
import java.util.regex.Pattern;

/** Actual local JVM derivative rendering/review/array delivery, not device/provider acceptance. */
public final class DerivativeExportBoundarySmoke {
    static void check(boolean value){if(!value)throw new AssertionError("derivative boundary contract failed");}
    static void rejected(Runnable action){try{action.run();throw new AssertionError("invalid derivative accepted");}catch(IllegalStateException error){check(error.getMessage().equals("Derivative export operation failed"));}}
    static String call(long host,String command){String value=NativeBridge.request(host,"{\"version\":1,\"command\":"+command+"}");check(value.contains("\"ok\":true"));return value;}
    static String id(String json){var m=Pattern.compile("\"id\":\"([^\"]+)\"").matcher(json);check(m.find());return m.group(1);}
    static long revision(long host,String id){var m=Pattern.compile("\"revision\":([0-9]+)").matcher(call(host,"{\"op\":\"Read\",\"id\":\""+id+"\"}"));check(m.find());return Long.parseLong(m.group(1));}
    static String select(String note){return "{\"Document\":\""+note+"\"}";}
    public static void run() throws Exception {
        long host=NativeBridge.open(Files.createTempDirectory("stem-derivative-export").resolve("library.db").toString());
        try {
            String note=id(call(host,"{\"op\":\"Create\",\"kind\":\"Note\",\"parent\":null,\"title\":\"derivative λ\"}"));
            call(host,"{\"op\":\"Append\",\"entity\":\""+note+"\",\"content\":{\"type\":\"Text\",\"value\":\"001 + 1 = 3 λ\"}}");
            call(host,"{\"op\":\"Capture\",\"ticket\":\""+UUID.randomUUID()+"\",\"entity\":\""+note+"\",\"strokes\":[{\"id\":\""+UUID.randomUUID()+"\",\"capture_session\":\""+UUID.randomUUID()+"\",\"sequence\":0,\"pointer_id\":1,\"coordinate_space\":\"synthetic-jni-derivative\",\"tool\":\"Pen\",\"points\":[{\"x\":1.2345678901234567,\"y\":2.0,\"time_us\":1,\"pressure\":null,\"tilt_rad\":null,\"orientation_rad\":null,\"buttons\":null}]}]}");
            try(var job=NativeBridge.reviewDerivativeExport(host,select(note),false)) {
                rejected(()->job.finish(c->true,true));
                String summary=job.renderMarkdown(30000);check(summary.contains("\"includes_original_ink\":false"));
                rejected(()->job.renderMarkdown(30000));
                rejected(()->NativeBridge.reviewDerivativeExport(host,select(note),false));
                try(var data=job.finish(c->true,true)){check(data.kind.equals("Markdown") && data.pdf==null && data.assets.length==0);String text=new String(data.markdown,StandardCharsets.UTF_8);check(text.contains("001 + 1 = 3 λ") && !text.contains("1.2345678901234567"));data.close();for(byte b:data.markdown)check(b==0);}
                rejected(()->job.finish(c->true,true));
            }
            try(var job=NativeBridge.reviewDerivativeExport(host,select(note),false)){job.renderMarkdown(30000);rejected(()->job.finish(c->true,false));}
            for(String invalid:new String[]{"{}","{\"Document\":\"bad\"}","{\"Document\":\""+note+"\",\"unknown\":1}"})rejected(()->NativeBridge.reviewDerivativeExport(host,invalid,false));
            System.out.println("PASS: JNI derivative render/review/confirmed Markdown delivery, exact authored text, explicit single use, admission and malformed selection (1)");
            try(var job=NativeBridge.reviewDerivativeExport(host,select(note),true)){
                check(job.renderMarkdown(30000).contains("\"includes_original_ink\":true"));
                try(var data=job.finish(c->true,true)){check(data.assets.length==1 && data.assetNames[0].startsWith("original-") && data.assetNames[0].endsWith(".svg"));check(new String(data.assets[0],StandardCharsets.UTF_8).contains("1.2345678901234567"));data.close();for(byte b:data.assets[0])check(b==0);}
            }
            System.out.println("PASS: JNI explicitly chosen original SVG asset delivery and owned binary-array retirement (1)");
            String configured=System.getenv("STEM_TEST_FONT");byte[] font=Files.readAllBytes(Path.of(configured==null?"/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf":configured));
            String page="{\"width_mm\":210,\"height_mm\":297,\"margin_mm\":12}";
            try(var job=NativeBridge.reviewDerivativeExport(host,select(note),false)){String summary=job.renderPdf(font,page,30000);check(summary.contains("typeset content derivative") && !summary.contains("\"pages\":null"));try(var data=job.finish(c->true,true)){check(data.kind.equals("Pdf") && data.markdown==null && data.assets.length==0 && new String(data.pdf,0,5,StandardCharsets.US_ASCII).equals("%PDF-"));}}
            try(var job=NativeBridge.reviewDerivativeExport(host,select(note),true)){rejected(()->job.renderPdf(font,page,30000));}
            try(var job=NativeBridge.reviewDerivativeExport(host,select(note),false)){rejected(()->job.renderPdf(new byte[]{1,2,3},page,30000));}
            try(var job=NativeBridge.reviewDerivativeExport(host,select(note),false)){rejected(()->job.renderPdf(font,"{\"unknown\":1}",30000));}
            System.out.println("PASS: JNI local-font PDF rendering, review fidelity warnings, original-ink refusal and malformed font/page containment (1)");
            NativeBridge.authenticate(host,"[{\"Entity\":\""+note+"\"}]",c->true);
            call(host,"{\"op\":\"SetLock\",\"entity\":\""+note+"\",\"revision\":"+revision(host,note)+",\"locked\":true}");
            try(var job=NativeBridge.reviewDerivativeExport(host,select(note),false)){job.renderMarkdown(30000);rejected(()->job.finish(c->false,true));}
            try(var job=NativeBridge.reviewDerivativeExport(host,select(note),false)){job.renderMarkdown(30000);try(var data=job.finish(c->{check(NativeBridge.captureLibraryIdentity(host).length()==36);return true;},true)){check(data.markdown.length>0);}}
            try(var job=NativeBridge.reviewDerivativeExport(host,select(note),false)){job.renderMarkdown(30000);call(host,"{\"op\":\"Append\",\"entity\":\""+note+"\",\"content\":{\"type\":\"Text\",\"value\":\"later\"}}");rejected(()->job.finish(c->true,true));}
            var cancelled=NativeBridge.reviewDerivativeExport(host,select(note),false);cancelled.renderMarkdown(30000);rejected(()->cancelled.finish(c->{cancelled.close();return true;},true));
            var relocked=NativeBridge.reviewDerivativeExport(host,select(note),false);relocked.renderMarkdown(30000);rejected(()->relocked.finish(c->{call(host,"{\"op\":\"Relock\"}");return true;},true));
            System.out.println("PASS: JNI derivative off-writer fresh proof, source-generation release guard, callback cancellation and relock refusal (1)");
        } finally {NativeBridge.close(host);}
    }
}
