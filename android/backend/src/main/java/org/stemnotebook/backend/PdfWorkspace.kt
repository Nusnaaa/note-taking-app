package org.stemnotebook.backend

import android.content.Context
import android.graphics.Canvas
import android.graphics.Paint
import android.graphics.RectF
import android.os.ParcelFileDescriptor
import android.os.StatFs
import org.json.JSONObject
import java.io.Closeable
import java.io.File
import java.io.FileOutputStream
import java.io.IOException
import java.security.MessageDigest
import java.util.UUID
import java.util.concurrent.ArrayBlockingQueue
import java.util.concurrent.CancellationException
import java.util.concurrent.CountDownLatch
import java.util.concurrent.RejectedExecutionException
import java.util.concurrent.ScheduledFuture
import java.util.concurrent.ScheduledThreadPoolExecutor
import java.util.concurrent.ThreadFactory
import java.util.concurrent.ThreadPoolExecutor
import java.util.concurrent.TimeUnit
import java.util.concurrent.TimeoutException

/** Backend-only source-policy pipeline. The source is linked; a disposable private
 * read cache freezes the exact verified revision for the isolated parser. It is not
 * a managed library copy, is never exported/backed up, and is unlinked before parse.
 * One job and one visible frame are retained; original notebook/ink are untouched. */
class PdfWorkspace internal constructor(
    private val directory: File,
    private val provider: PdfProvider,
    private val parser: PdfParserClient,
    private val native: (JSONObject) -> JSONObject,
) : Closeable {
    internal constructor(context: Context, native: (JSONObject) -> JSONObject) : this(
        File(context.noBackupFilesDir, "pdf-read-cache"), PdfProvider(context), PdfParserClient(context), native,
    )
    private val lock = Any()
    private var epoch = 0L
    private var closed = false
    private var current: Operation? = null
    private var displayed: Frame? = null
    private fun factory(name: String) = ThreadFactory { Thread(it, name).apply { isDaemon = true } }
    private val worker = ThreadPoolExecutor(1, 1, 0, TimeUnit.MILLISECONDS, ArrayBlockingQueue(1), factory("pdf-source"), ThreadPoolExecutor.AbortPolicy())
    private val cleanup = ThreadPoolExecutor(1, 1, 0, TimeUnit.MILLISECONDS, ArrayBlockingQueue(1), factory("pdf-source-dispose"), ThreadPoolExecutor.AbortPolicy())
    private val deadlines = ScheduledThreadPoolExecutor(1, factory("pdf-source-deadline")).apply { removeOnCancelPolicy = true }
    init {
        check(directory.isDirectory || directory.mkdirs())
        require(!java.nio.file.Files.isSymbolicLink(directory.toPath()))
        // Crash leftovers are disposable live bytes, never independently retained evidence.
        directory.listFiles()?.filter { it.name.startsWith("pdf-") && it.name.endsWith(".pending") }?.forEach { check(it.delete()) }
    }
    fun render(source: UUID, page: Int, width: Int, height: Int, timeoutMillis: Long = 30_000): PendingFrame {
        return submit(source, false, page, width, height, timeoutMillis)
    }
    fun renderAsset(asset: UUID, page: Int, width: Int, height: Int, timeoutMillis: Long = 30_000): PendingFrame = submit(asset, true, page, width, height, timeoutMillis)
    private fun submit(source: UUID, asset: Boolean, page: Int, width: Int, height: Int, timeoutMillis: Long): PendingFrame {
        PdfPixels.validate(width, height); require(page >= 0 && timeoutMillis in 1..120_000)
        val operation = synchronized(lock) {
            check(!closed)
            if (current != null) throw RejectedExecutionException("PDF source pipeline busy")
            Operation(source, asset, page, width, height, timeoutMillis, epoch).also { current = it }
        }
        operation.start()
        return PendingFrame(operation)
    }
    /** Call before enqueuing any source/lock/containment policy change, including
     * unsuccessful attempts. Retirement does not wait for a provider or parser. */
    fun invalidate() {
        val resources = synchronized(lock) { epoch++; val value = current to displayed; displayed = null; value }
        resources.second?.close()
        resources.first?.fail(CancellationException("PDF source policy changed"))
    }
    override fun close() {
        synchronized(lock) { if (closed) return; closed = true }
        invalidate()
        provider.close(); parser.close()
        worker.shutdown(); cleanup.shutdown(); deadlines.shutdownNow()
    }
    private fun command(op: String, fields: JSONObject = JSONObject()): JSONObject {
        fields.put("op", op)
        val result = native(JSONObject().put("op", "Pdf").put("command", fields))
        if (result.getInt("version") != 1 || !result.getBoolean("ok")) throw IOException("PDF source operation unavailable")
        return result.optJSONObject("data") ?: JSONObject()
    }
    class PendingFrame internal constructor(private val operation: Operation) : Closeable {
        /** Blocking; use off the Android main thread. */
        fun await(): Frame = operation.take()
        val isReady: Boolean get() = operation.isReady()
        fun cancel() { operation.fail(CancellationException("PDF reading cancelled")) }
        override fun close() = cancel()
    }
    /** Owns a revocable viewport, not an export capability. No Bitmap escapes this
     * object. A later publication or policy change retires the earlier frame. */
    class Frame internal constructor(bitmap: android.graphics.Bitmap, val pages: Int, val sourceRevision: String, private var expires: Long, val retainedAsset: UUID? = null, val historical: Boolean = false) : Closeable {
        private val lock = Any()
        private var pixels: android.graphics.Bitmap? = bitmap
        private var expiry: ScheduledFuture<*>? = null
        internal fun limitValidity(until: Long) = synchronized(lock) { expires = minOf(expires, until) }
        internal fun expireWith(timer: ScheduledFuture<*>) = synchronized(lock) {
            if (pixels == null) timer.cancel(false) else expiry = timer
        }
        private fun current(): android.graphics.Bitmap? {
            if (System.nanoTime() >= expires) { pixels?.recycle(); pixels = null; expiry?.cancel(false) }
            return pixels
        }
        val width: Int = bitmap.width
        val height: Int = bitmap.height
        fun draw(canvas: Canvas, destination: RectF, paint: Paint? = null) = synchronized(lock) {
            val bitmap = current() ?: throw IllegalStateException("PDF preview retired")
            canvas.drawBitmap(bitmap, null, destination, paint)
        }
        val retired: Boolean get() = synchronized(lock) { current() == null }
        override fun close() { synchronized(lock) { pixels?.recycle(); pixels = null; expiry?.cancel(false) } }
    }
    internal inner class Operation(private val source: UUID, private val asset: Boolean, private val page: Int, private val width: Int, private val height: Int, private val timeoutMillis: Long, private val generation: Long) {
        private val state = Any()
        private val ready = CountDownLatch(1)
        private var failure: Exception? = null
        private var frame: Frame? = null
        private var claimed = false
        private var finished = false
        private var cleanupPending = false
        private var deadline: ScheduledFuture<*>? = null
        private var policyDeadline: ScheduledFuture<*>? = null
        private var opening: PdfProvider.PendingPdf? = null
        private var parsing: PdfParserClient.PendingBitmap? = null
        private var reading: ParcelFileDescriptor? = null
        private var cache: File? = null
        private val started = System.nanoTime()
        fun start() {
            try {
                val timer = deadlines.schedule({ fail(TimeoutException("PDF reading timed out")) }, timeoutMillis, TimeUnit.MILLISECONDS)
                synchronized(state) { if (failure != null) timer.cancel(false) else deadline = timer }
                worker.execute { run() }
            } catch (_: RejectedExecutionException) { fail(IOException("PDF source pipeline closed")); finish() }
        }
        private fun checkpoint() {
            synchronized(state) { failure?.let { throw it } }
            synchronized(lock) { if (closed || epoch != generation) throw CancellationException("PDF source policy changed") }
            if (System.nanoTime() - started >= TimeUnit.MILLISECONDS.toNanos(timeoutMillis)) throw TimeoutException("PDF reading timed out")
        }
        private fun remaining(): Long = maxOf(1, timeoutMillis - TimeUnit.NANOSECONDS.toMillis(System.nanoTime() - started))
        private fun run() {
            var lease: String? = null
            var pendingFrame: Frame? = null
            try {
                checkpoint()
                val preparing = System.nanoTime()
                val plan = if (asset) command("PrepareAssetRead", JSONObject().put("asset", source.toString())) else command("PrepareRead", JSONObject().put("source", source.toString()))
                lease = plan.getString("lease")
                val metadata = if (asset) plan.getJSONObject("asset") else plan
                val expected = metadata.getString(if (asset) "digest" else "revision")
                val sourceRevision = metadata.getString(if (asset) "source_revision" else "revision")
                val historical = asset && !metadata.isNull("retention")
                fun validate(): JSONObject = command(if (asset) "ValidateAssetRead" else "ValidateRead", JSONObject().put("lease", lease).put(if (asset) "observed_digest" else "observed_revision", expected))
                val validity = plan.getLong("valid_for_ms")
                if (validity <= 0) throw CancellationException("PDF source access expired")
                val expires = preparing + TimeUnit.MILLISECONDS.toNanos(validity.coerceAtMost(300_000))
                val expiry = deadlines.schedule({ fail(CancellationException("PDF source access expired")) }, maxOf(0, expires - System.nanoTime()), TimeUnit.NANOSECONDS)
                synchronized(state) { if (failure != null) expiry.cancel(false) else policyDeadline = expiry }
                checkpoint()
                val uri = if (asset) null else android.net.Uri.parse(plan.getString("locator"))
                val file = File.createTempFile("pdf-", ".pending", directory)
                synchronized(state) { cache = file }
                val hash = MessageDigest.getInstance("SHA-256")
                FileOutputStream(file).use { output ->
                    var bytes = 0L
                    fun write(chunk: ByteArray, count: Int) {
                        checkpoint()
                        bytes += count
                        if (bytes > 16L * 1024 * 1024 * 1024 || StatFs(directory.path).availableBytes < 8L * 1024 * 1024 + count) throw IOException("PDF temporary storage unavailable")
                        hash.update(chunk, 0, count); output.write(chunk, 0, count)
                    }
                    if (asset) {
                        val length = metadata.getLong("length")
                        if (length !in 5..64L * 1024 * 1024) throw IOException("PDF asset size unavailable")
                        while (bytes < length) {
                            checkpoint()
                            val count = minOf(65536L, length - bytes).toInt()
                            val chunk = command("ReadAssetChunk", JSONObject().put("lease", lease).put("offset", bytes).put("length", count))
                            val values = chunk.getJSONArray("bytes")
                            if (chunk.getLong("offset") != bytes || values.length() != count) throw IOException("PDF asset chunk unavailable")
                            val buffer = ByteArray(count) { index ->
                                val value = values.getInt(index)
                                if (value !in 0..255) throw IOException("PDF asset chunk unavailable")
                                value.toByte()
                            }
                            write(buffer, count)
                        }
                    } else {
                        val pending = provider.openReadOnly(uri!!, remaining())
                        val cancelOpening = synchronized(state) { opening = pending; failure != null }
                        if (cancelOpening) pending.cancel()
                        checkpoint()
                        val fd = pending.await()
                        synchronized(state) { opening = null; reading = fd }
                        checkpoint()
                        ParcelFileDescriptor.AutoCloseInputStream(fd).use { input ->
                            val chunk = ByteArray(65536)
                            while (true) {
                                checkpoint()
                                val count = input.read(chunk)
                                if (count < 0) break
                                write(chunk, count)
                            }
                        }
                        synchronized(state) { reading = null }
                    }
                    output.fd.sync()
                }
                val observed = hash.digest().joinToString("") { "%02x".format(it.toInt() and 255) }
                if (!observed.equals(expected, ignoreCase = true)) throw IOException("PDF revision changed; review required")
                checkpoint()
                validate()
                ParcelFileDescriptor.open(file, ParcelFileDescriptor.MODE_READ_ONLY).use { frozen ->
                    check(file.delete())
                    synchronized(state) { cache = null }
                    checkpoint()
                    val request = parser.render(frozen, page, width, height, remaining())
                    val cancelParsing = synchronized(state) { parsing = request; failure != null }
                    if (cancelParsing) request.cancel()
                    checkpoint()
                    val parsed = request.await()
                    synchronized(state) { parsing = null }
                    // Keep ownership private until the final native validation.
                    pendingFrame = Frame(parsed.bitmap, parsed.pages, sourceRevision, Long.MAX_VALUE, if (asset) source else null, historical)
                }
                checkpoint()
                // An old open descriptor is not evidence that the provider still
                // permits live access. Re-open read-only before publishing pixels.
                if (!asset) {
                    val grant = provider.openReadOnly(uri!!, remaining())
                    val cancelGrant = synchronized(state) { opening = grant; failure != null }
                    if (cancelGrant) grant.cancel()
                    checkpoint()
                    grant.await().close()
                    synchronized(state) { opening = null }
                    checkpoint()
                }
                val validating = System.nanoTime()
                val checked = validate()
                val until = validating + TimeUnit.MILLISECONDS.toNanos(checked.getLong("valid_for_ms").coerceIn(0, 300_000))
                // Apply an earlier deadline to the same privately owned bitmap.
                pendingFrame!!.limitValidity(until)
                val expiringFrame = pendingFrame!!
                expiringFrame.expireWith(deadlines.schedule({ expiringFrame.close() }, maxOf(0, until - System.nanoTime()), TimeUnit.NANOSECONDS))
                checkpoint()
                // Native policy requests pass through NotebookBackend.invalidate;
                // publish under that same workspace epoch, never from an old callback.
                val retired = synchronized(lock) {
                    synchronized(state) {
                        failure?.let { throw it }
                        if (closed || epoch != generation || pendingFrame!!.retired) throw CancellationException("PDF source policy changed")
                        val previous = displayed
                        displayed = pendingFrame
                        frame = pendingFrame; pendingFrame = null
                        previous
                    }
                }
                retired?.close()
            } catch (error: Exception) {
                fail(when (error) {
                    is TimeoutException -> TimeoutException("PDF reading timed out")
                    is CancellationException -> CancellationException("PDF reading cancelled")
                    else -> IOException("PDF source unavailable or revision needs review")
                })
            } finally {
                pendingFrame?.close()
                val resources = synchronized(state) { val result = Triple(opening, reading, cache); opening = null; reading = null; cache = null; result }
                val abandonedParser = synchronized(state) { parsing.also { parsing = null } }
                try { resources.first?.close() } catch (_: Exception) { }
                try { abandonedParser?.close() } catch (_: Exception) { }
                try { resources.second?.close() } catch (_: Exception) { }
                resources.third?.delete()
                try { resources.first?.awaitRetired() } catch (_: InterruptedException) { Thread.currentThread().interrupt() }
                try { abandonedParser?.awaitRetired() } catch (_: InterruptedException) { Thread.currentThread().interrupt() }
                lease?.let { try { command("ReleaseRead", JSONObject().put("lease", it)) } catch (_: Exception) { } }
                finish()
            }
        }
        fun fail(error: Exception) {
            val resources: Triple<PdfProvider.PendingPdf?, PdfParserClient.PendingBitmap?, ParcelFileDescriptor?>
            val ownedFrame: Frame?
            val needsCleanup: Boolean
            synchronized(state) {
                if (failure != null || claimed) return
                failure = error; deadline?.cancel(false); policyDeadline?.cancel(false)
                resources = Triple(opening, parsing, reading)
                ownedFrame = frame; frame = null
                needsCleanup = resources.first != null || resources.second != null || resources.third != null || cache != null
                cleanupPending = needsCleanup
                ready.countDown()
            }
            ownedFrame?.close()
            val oldFrame = synchronized(lock) {
                // A cancelled old receipt must never clear a newer viewport.
                if (current === this || (ownedFrame != null && displayed === ownedFrame)) {
                    val value = displayed; displayed = null; value
                } else null
            }
            oldFrame?.close()
            if (!needsCleanup) { releaseFinished(); return }
            fun dispose() {
                try {
                    try { resources.first?.cancel() } finally {
                        try { resources.second?.cancel() } finally { resources.third?.close() }
                    }
                } catch (_: Exception) { /* terminal content-free failure already published */ }
                finally {
                    synchronized(state) { cache }?.delete()
                    synchronized(state) { cleanupPending = false }
                    releaseFinished()
                }
            }
            try { cleanup.execute { dispose() } } catch (_: RejectedExecutionException) { dispose() }
        }
        private fun releaseFinished() {
            val release = synchronized(state) { finished && !cleanupPending }
            if (release) synchronized(lock) { if (current === this) current = null }
        }
        private fun finish() {
            synchronized(state) { finished = true; deadline?.cancel(false); policyDeadline?.cancel(false) }
            releaseFinished()
            ready.countDown()
        }
        fun isReady(): Boolean = ready.count == 0L
        fun take(): Frame {
            try { ready.await() } catch (interrupted: InterruptedException) {
                fail(CancellationException("PDF reading interrupted")); Thread.currentThread().interrupt(); throw interrupted
            }
            return synchronized(state) {
                failure?.let { throw it }
                check(finished && !claimed)
                claimed = true
                frame?.takeUnless { it.retired } ?: throw IOException("PDF preview unavailable")
            }
        }
    }
}
