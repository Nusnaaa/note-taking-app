package org.stemnotebook.backend

import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TemporaryFolder
import org.junit.runner.RunWith
import org.robolectric.annotation.Config
import java.io.ByteArrayOutputStream
import java.lang.reflect.InvocationTargetException
import java.util.concurrent.CountDownLatch
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit
import java.util.concurrent.TimeoutException

/** Desktop JNI in the API-35 harness; no device or public-provider durability claim. */
@RunWith(JniBackendRunner::class)
@Config(sdk = [35])
class NativeBackupBoundaryTest {
    @get:Rule val temporary = TemporaryFolder()
    private val password get() = "Deliberate native backup test passphrase".toCharArray()
    private fun open() = NativeBridge.open(temporary.newFolder().resolve("library.db").absolutePath)
    private fun rejected(action: () -> Unit) {
        try { action(); fail("accepted invalid backup") } catch (expected: IllegalStateException) {
            assertFalse(expected.message.orEmpty().contains("private"))
        }
    }
    private open class Sink : NativeBridge.BackupOutput {
        val bytes = ByteArrayOutputStream()
        var retired = 0
        var max = 0
        override fun write(ciphertext: ByteArray) { max = maxOf(max, ciphertext.size); bytes.write(ciphertext) }
        override fun flush() = Unit
        override fun retire() { retired++ }
    }
    @Test(timeout = 30000) fun ciphertextSummaryKeyAndSingleUseOwnership() {
        val host = open()
        try {
            val review = NativeBridge.reviewBackup(host, true, false)
            assertFalse(review.includesOriginalInk)
            assertEquals(0L, review.sourceAssets)
            val job = review.begin(20000)
            val sink = Sink()
            val supplied = password
            val key = job.run(supplied, { true }, true, sink)
            try {
                assertEquals(64, key.size)
                assertArrayEquals(password, supplied)
                assertEquals("STEMENC2", sink.bytes.toByteArray().copyOfRange(0, 8).toString(Charsets.US_ASCII))
                assertTrue(sink.max <= 65552)
                assertEquals(1, sink.retired)
                rejected { job.run(supplied, { true }, true, Sink()) }
                rejected { review.begin(20000) }
            } finally { key.fill('\u0000'); supplied.fill('\u0000'); job.close(); review.close() }
        } finally { NativeBridge.close(host) }
    }
    @Test(timeout = 30000) fun protectedBackupRequiresCurrentAccessFreshProofAndConfirmation() {
        val host = open()
        try {
            val created = JSONObject(NativeBridge.request(host, """{"version":1,"command":{"op":"Create","kind":"Note","parent":null,"title":"private fixture"}}"""))
            val id = created.getJSONObject("data").getString("id")
            val scopes = """[{"Entity":"$id"}]"""
            NativeBridge.authenticate(host, scopes) { true }
            assertTrue(JSONObject(NativeBridge.request(host, """{"version":1,"command":{"op":"SetLock","entity":"$id","revision":0,"locked":true}}""")).getBoolean("ok"))
            NativeBridge.request(host, """{"version":1,"command":{"op":"Relock"}}""")
            rejected { NativeBridge.reviewBackup(host, true, false) }
            NativeBridge.authenticate(host, scopes) { true }
            for (confirmed in listOf(false, true)) {
                val sink = Sink()
                NativeBridge.reviewBackup(host, true, false).begin(20000).use { job ->
                    rejected { job.run(password, { throw IllegalArgumentException("private verifier diagnostic") }, confirmed, sink) }
                }
                assertEquals(0, sink.bytes.size()); assertEquals(1, sink.retired)
            }
            var calls = 0
            NativeBridge.reviewBackup(host, true, false).begin(20000).use { job ->
                job.run(password, { calls++; true }, true, Sink()).fill('\u0000')
            }
            assertEquals(1, calls)
        } finally { NativeBridge.close(host) }
    }
    @Test(timeout = 30000) fun foreignNativeTokensCannotConsumeOwnersReview() {
        val owner = open(); val foreign = open()
        try {
            NativeBridge.reviewBackup(owner, true, false).use { review ->
                val token = review.javaClass.getDeclaredField("token").apply { isAccessible = true }.getLong(review)
                val release = NativeBridge::class.java.getDeclaredMethod("releaseBackup", java.lang.Long.TYPE, java.lang.Long.TYPE).apply { isAccessible = true }
                try { release.invoke(null, foreign, token); fail("foreign token accepted") }
                catch (error: InvocationTargetException) { assertTrue(error.cause is IllegalStateException) }
                review.begin(20000).use { it.run(password, { true }, true, Sink()).fill('\u0000') }
            }
        } finally { NativeBridge.close(owner); NativeBridge.close(foreign) }
    }
    @Test(timeout = 30000) fun invalidPasswordAndSinkFailureRetireExactlyOnceWithoutKey() {
        val host = open()
        try {
            val sinks = listOf(Sink(), object : Sink() {
                override fun write(ciphertext: ByteArray) { throw IllegalArgumentException("private output diagnostic") }
            }, object : Sink() {
                override fun retire() { super.retire(); throw IllegalArgumentException("private retirement diagnostic") }
            }, Sink())
            sinks.forEachIndexed { index, sink ->
                NativeBridge.reviewBackup(host, true, false).begin(20000).use { job ->
                    rejected { job.run(when (index) { 0 -> CharArray(1025); 3 -> charArrayOf('\uD800'); else -> password }, { true }, true, sink) }
                }
                assertEquals(1, sink.retired)
            }
            assertTrue(JSONObject(NativeBridge.request(host, """{"version":1,"command":{"op":"Status"}}""")).getBoolean("ok"))
        } finally { NativeBridge.close(host) }
    }
    @Test(timeout = 30000) fun callbackCloseDoesNotSelfJoinOrReturnRecoveryKey() {
        val path = temporary.newFolder().resolve("library.db").absolutePath
        val host = NativeBridge.open(path)
        val sink = object : Sink() {
            override fun write(ciphertext: ByteArray) { NativeBridge.close(host) }
        }
        NativeBridge.reviewBackup(host, true, false).begin(20000).use { job ->
            rejected { job.run(password, { true }, true, sink) }
        }
        assertEquals(1, sink.retired)
        NativeBridge.close(NativeBridge.open(path))
    }
    @Test(timeout = 45000) fun cancelledRetiringCallbackRetainsNativeOwnerAndLeavesWriterFree() {
        val path = temporary.newFolder().resolve("library.db").absolutePath
        val host = NativeBridge.open(path)
        val entered = CountDownLatch(1); val release = CountDownLatch(1)
        val executor = Executors.newFixedThreadPool(2)
        val job = NativeBridge.reviewBackup(host, true, false).begin(30000)
        val sink = object : Sink() {
            override fun retire() { super.retire(); entered.countDown(); check(release.await(30, TimeUnit.SECONDS)) }
        }
        val work = executor.submit { rejected { job.run(password, { true }, true, sink) } }
        var closing: java.util.concurrent.Future<*>? = null
        try {
            assertTrue(entered.await(30, TimeUnit.SECONDS))
            assertTrue(JSONObject(NativeBridge.request(host, """{"version":1,"command":{"op":"Status"}}""")).getBoolean("ok"))
            job.close(); closing = executor.submit { NativeBridge.close(host) }
            try { closing.get(150, TimeUnit.MILLISECONDS); fail("premature close") } catch (_: TimeoutException) { }
            rejected { NativeBridge.open(path) }
        } finally {
            release.countDown(); work.get(10, TimeUnit.SECONDS)
            if (closing != null) closing.get(10, TimeUnit.SECONDS) else NativeBridge.close(host)
            executor.shutdownNow()
        }
        assertEquals(1, sink.retired)
        NativeBridge.close(NativeBridge.open(path))
    }
}
