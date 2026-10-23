package org.stemnotebook.backend

import android.graphics.Bitmap
import android.os.*
import org.junit.Assert.*
import java.util.concurrent.CountDownLatch
import java.util.concurrent.Executor
import java.util.concurrent.atomic.AtomicInteger

internal class FixturePdfConnection(private val callbacks: Executor, val mode: String) : PdfConnection {
        val entered = CountDownLatch(1)
        val release = CountDownLatch(1)
        val closes = AtomicInteger(0)
        var source: ParcelFileDescriptor? = null
        var output: ParcelFileDescriptor? = null
        var failed: (() -> Unit)? = null
        var connected: ((Messenger) -> Unit)? = null
        val messenger = Messenger(object : Handler(Looper.getMainLooper()) {
            // The same-process Messenger skips parcel FD duplication. Duplicate at
            // synchronous admission to model the real cross-process Binder contract.
            @Suppress("DEPRECATION")
            override fun sendMessageAtTime(message: Message, uptimeMillis: Long): Boolean {
                if (message.what == 99) return true
                if (mode == "blocked-send") { entered.countDown(); release.await(); return true }
                val fd = message.data.getParcelable<ParcelFileDescriptor>("descriptor")!!
                source = ParcelFileDescriptor.dup(fd.fileDescriptor)
                entered.countDown()
                assertEquals(setOf("descriptor", "width", "height", "format"), message.data.keySet())
                if (mode == "death") return true
                val pipe = ParcelFileDescriptor.createPipe()
                // Robolectric's pipe is a regular file and dup copies its current
                // bytes, rather than duplicating a live kernel pipe. Fill the tiny
                // fixture first; actual pipe streaming/device acceptance is separate.
                ParcelFileDescriptor.AutoCloseOutputStream(pipe[1]).use { stream ->
                    if (mode == "truncated") stream.write(byteArrayOf(1, 2, 3))
                    else if (mode != "wrong-token") {
                        val bitmap = Bitmap.createBitmap(2, 2, Bitmap.Config.ARGB_8888)
                        try { bitmap.eraseColor(0xff112233.toInt()); PdfPixels.write(bitmap, stream) } finally { bitmap.recycle() }
                    }
                }
                output = ParcelFileDescriptor.dup(pipe[0].fileDescriptor)
                val response = Message.obtain(null, 1).apply {
                    arg2 = if (mode == "wrong-token") message.arg2 + 1 else message.arg2
                    data = Bundle().apply { putParcelable("pixels", output); putInt("pages", 3); putString("format", PdfPixels.FORMAT); putBoolean("ok", true) }
                }
                message.replyTo.send(response)
                pipe[0].close()
                source?.close()
                return true
            }
        })
        override fun connect(ready: (Messenger) -> Unit, failed: () -> Unit): Boolean {
            this.failed = failed; connected = ready
            if (mode != "never-bind") callbacks.execute { ready(messenger) }
            return true
        }
        override fun close() {
            closes.incrementAndGet(); source?.close()
            if (mode == "close-fails") error("Synthetic disposal failure")
        }
    }
