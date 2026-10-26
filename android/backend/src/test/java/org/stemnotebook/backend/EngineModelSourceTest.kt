package org.stemnotebook.backend

import android.content.ContentProvider
import android.content.ContentValues
import android.content.Intent
import android.content.pm.ProviderInfo
import android.database.Cursor
import android.net.Uri
import android.os.ParcelFileDescriptor
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TemporaryFolder
import org.junit.runner.RunWith
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config
import org.robolectric.shadows.ShadowContentResolver
import java.io.File
import java.util.concurrent.Callable
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicInteger

@RunWith(JniBackendRunner::class)
@Config(sdk = [35])
class EngineModelSourceTest {
    @get:Rule val temporary=TemporaryFolder()
    private fun <T> worker(action:()->T):T {val executor=Executors.newSingleThreadExecutor();return try {executor.submit(Callable{action()}).get(30,TimeUnit.SECONDS)}finally{executor.shutdownNow()}}
    private class Provider(val file:File):ContentProvider() {
        val opens=AtomicInteger();var last:ParcelFileDescriptor?=null
        override fun onCreate()=true
        override fun query(uri:Uri,p:Array<out String>?,s:String?,a:Array<out String>?,o:String?):Cursor?=null
        override fun getType(uri:Uri)="application/octet-stream"
        override fun insert(uri:Uri,v:ContentValues?):Uri?=throw UnsupportedOperationException()
        override fun delete(uri:Uri,s:String?,a:Array<out String>?)=throw UnsupportedOperationException()
        override fun update(uri:Uri,v:ContentValues?,s:String?,a:Array<out String>?)=throw UnsupportedOperationException()
        override fun openFile(uri:Uri,mode:String):ParcelFileDescriptor {assertEquals("r",mode);opens.incrementAndGet();return ParcelFileDescriptor.open(file,ParcelFileDescriptor.MODE_READ_ONLY).also{last=it}}
    }
    @Test fun pickerRequiresContentReadObservationWithoutOpeningOrPersistingPermission() {
        val context=RuntimeEnvironment.getApplication()
        for(intent in listOf(Intent().setData(Uri.parse("file:///private/model" )).addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION),Intent().setData(Uri.parse("content://model-source/one")))) {
            assertThrows(NotebookEngines.Failure::class.java){EngineModelSource.fromPicker(context,intent)}
        }
        val before=context.contentResolver.persistedUriPermissions.size
        EngineModelSource.fromPicker(context,Intent().setData(Uri.parse("content://model-source/one")).addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION))
        assertEquals(before,context.contentResolver.persistedUriPermissions.size)
    }
    @Test fun selectedReadOnlyProviderResumesAtAcknowledgedOffsetAndClosesItsActualDescriptor() {
        val context=RuntimeEnvironment.getApplication();val sourceFile=temporary.newFile("selected.model");val bytes=ByteArray(70001){(it*31+137).toByte()};sourceFile.writeBytes(bytes)
        val provider=Provider(sourceFile);val authority="org.stemnotebook.fixture.modelsource";provider.attachInfo(context,ProviderInfo().apply{this.authority=authority;exported=true});ShadowContentResolver.registerProviderInternal(authority,provider)
        val source=EngineModelSource.fromPicker(context,Intent(Intent.ACTION_OPEN_DOCUMENT).setData(Uri.parse("content://$authority/selected")).addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION))
        val host=NativeBridge.open(temporary.newFolder().resolve("library.db").absolutePath);val models=temporary.newFolder("models");val engines=NotebookEngines(models){host}
        val sha=java.security.MessageDigest.getInstance("SHA-256").digest(bytes).joinToString(""){"%02x".format(it.toInt() and 255)}
        val manifest=JSONObject().put("id","synthetic-picker").put("version","1").put("adapter_version","1").put("family","Recogniser").put("input","OrderedStrokesV1").put("tasks",org.json.JSONArray().put("Prose")).put("internals","StrokeOnlyVerified").put("offline",true).put("content_egress",false).put("personal_use_evidence","app-owned synthetic picker fixture").put("redistributable_resources",false).put("resource_sha256",sha).put("runtime_contract","stem-processing-v1").toString()
        val receipt=java.util.concurrent.atomic.AtomicReference<NotebookEngines.Transfer>();val retired=AtomicInteger()
        try {
            val partial=engines.importResource(manifest,bytes.size.toLong(),true,null,30000,{receipt.set(it)}) {_,_->object:NativeBridge.EngineInput {
                var read=false
                override fun read(buffer:ByteArray,control:NativeBridge.EngineControl):Int {control.check();if(read)throw IllegalArgumentException("private fixture read failure");read=true;bytes.copyInto(buffer,0,0,buffer.size);return buffer.size}
                override fun retire(){retired.incrementAndGet()}
            }}
            assertEquals(NotebookEngines.FailureReason.Unavailable,worker {try{partial.await();throw AssertionError()}catch(error:NotebookEngines.Failure){error.reason}});assertEquals(1,retired.get())
            val resumed=worker {engines.importResource(manifest,bytes.size.toLong(),true,receipt.get().version,30000,source){assertEquals(65536L,it.committedBytes)}.await()}
            assertEquals(receipt.get().version,resumed.version);assertEquals(1,provider.opens.get());assertArrayEquals(bytes,File(models,"${resumed.version}.resource").readBytes())
            assertThrows(IllegalStateException::class.java){provider.last!!.fd}
            assertArrayEquals(bytes,sourceFile.readBytes())
        } finally {engines.close();NativeBridge.close(host);worker{engines.awaitClosed()}}
    }
}
