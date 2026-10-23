package org.stemnotebook.backend

import android.graphics.Bitmap
import java.io.BufferedInputStream
import java.io.BufferedOutputStream
import java.io.DataInputStream
import java.io.DataOutputStream
import java.io.IOException
import java.io.InputStream
import java.io.OutputStream

/** Private parser IPC v1: exact big-endian ARGB rows, no image decoder in the host.
 * Header dimensions are checked against the requested bounded viewport before
 * allocation. Truncation and trailing bytes fail; successful publication needs EOF. */
internal object PdfPixels {
    const val FORMAT = "stem-argb-1"
    private val magic = "STEMPIX1".toByteArray(Charsets.US_ASCII)
    fun validate(width: Int, height: Int) {
        require(width in 1..4096 && height in 1..4096 && width.toLong() * height <= 8_000_000)
    }
    fun write(bitmap: Bitmap, output: OutputStream) {
        validate(bitmap.width, bitmap.height)
        val wire = DataOutputStream(BufferedOutputStream(output, 65536))
        wire.write(magic); wire.writeInt(bitmap.width); wire.writeInt(bitmap.height)
        val row = IntArray(bitmap.width)
        for (y in 0 until bitmap.height) {
            bitmap.getPixels(row, 0, bitmap.width, 0, y, bitmap.width, 1)
            for (pixel in row) wire.writeInt(pixel)
        }
        wire.flush()
    }
    fun read(input: InputStream, width: Int, height: Int, checkpoint: () -> Unit = {}): Bitmap {
        validate(width, height)
        val wire = DataInputStream(BufferedInputStream(input, 65536))
        val header = ByteArray(magic.size)
        wire.readFully(header)
        if (!header.contentEquals(magic) || wire.readInt() != width || wire.readInt() != height) throw IOException("Invalid PDF pixel frame")
        val bitmap = Bitmap.createBitmap(width, height, Bitmap.Config.ARGB_8888)
        try {
            val row = IntArray(width)
            for (y in 0 until height) {
                checkpoint()
                for (x in 0 until width) row[x] = wire.readInt()
                bitmap.setPixels(row, 0, width, 0, y, width, 1)
            }
            if (wire.read() != -1) throw IOException("Unexpected PDF pixel bytes")
            checkpoint()
            return bitmap
        } catch (error: Throwable) { bitmap.recycle(); throw error }
    }
}
