package ai.mukei.android.core.nativebridge

import android.content.Context
import android.graphics.Bitmap
import android.graphics.Color
import android.graphics.pdf.PdfRenderer
import android.net.Uri
import android.os.ParcelFileDescriptor
import com.google.android.gms.tasks.Tasks
import com.google.mlkit.vision.common.InputImage
import com.google.mlkit.vision.text.TextRecognizer
import com.google.mlkit.vision.text.TextRecognition
import com.google.mlkit.vision.text.devanagari.DevanagariTextRecognizerOptions
import org.json.JSONObject
import java.io.File
import java.util.Locale
import java.util.concurrent.TimeUnit
import kotlin.math.roundToInt
import kotlin.math.sqrt

/**
 * Bounded, on-device OCR for staged user documents.
 *
 * OCR is intentionally best-effort: document staging must remain successful when
 * recognition is unsupported, empty, slow, or resource constrained. Recognized
 * text is returned in-memory to Rust and is never written as a plaintext sidecar.
 */
internal object AndroidOcr {
    fun extract(
        context: Context,
        sourceUri: Uri,
        stagedFile: File,
        mimeType: String,
    ): JSONObject {
        val normalizedMime = mimeType.trim().lowercase(Locale.ROOT)
        return try {
            when {
                normalizedMime.startsWith("image/") -> recognizeImage(context, sourceUri)
                normalizedMime == PDF_MIME_TYPE -> recognizePdf(stagedFile)
                else -> baseResult("unsupported")
            }
        } catch (_: OutOfMemoryError) {
            failedResult("ocr_resource_exhausted")
        } catch (_: Exception) {
            failedResult("ocr_processing_failed")
        }
    }

    private fun recognizeImage(context: Context, sourceUri: Uri): JSONObject =
        withRecognizer { recognizer ->
            val image = InputImage.fromFilePath(context, sourceUri)
            val text = Tasks.await(
                recognizer.process(image),
                OCR_TASK_TIMEOUT_SECONDS,
                TimeUnit.SECONDS,
            ).text
            textResult(
                text = text,
                pagesProcessed = 1,
                sourcePages = 1,
                truncated = false,
            )
        }

    private fun recognizePdf(stagedFile: File): JSONObject {
        val descriptor = ParcelFileDescriptor.open(stagedFile, ParcelFileDescriptor.MODE_READ_ONLY)
        try {
            val renderer = PdfRenderer(descriptor)
            try {
                val sourcePages = renderer.pageCount
                if (sourcePages == 0) {
                    return textResult("", 0, 0, false)
                }
                return withRecognizer { recognizer ->
                    val output = StringBuilder()
                    val pagesToProcess = minOf(sourcePages, MAX_PDF_PAGES)
                    var characterTruncated = false
                    for (index in 0 until pagesToProcess) {
                        val page = renderer.openPage(index)
                        try {
                            val (width, height) = boundedRenderSize(page.width, page.height)
                            val bitmap = Bitmap.createBitmap(width, height, Bitmap.Config.ARGB_8888)
                            try {
                                bitmap.eraseColor(Color.WHITE)
                                page.render(
                                    bitmap,
                                    null,
                                    null,
                                    PdfRenderer.Page.RENDER_MODE_FOR_DISPLAY,
                                )
                                val recognized = Tasks.await(
                                    recognizer.process(InputImage.fromBitmap(bitmap, 0)),
                                    OCR_TASK_TIMEOUT_SECONDS,
                                    TimeUnit.SECONDS,
                                ).text
                                if (appendBounded(output, recognized, index + 1)) {
                                    characterTruncated = true
                                    break
                                }
                            } finally {
                                bitmap.recycle()
                            }
                        } finally {
                            page.close()
                        }
                    }
                    textResult(
                        text = output.toString(),
                        pagesProcessed = pagesToProcess,
                        sourcePages = sourcePages,
                        truncated = characterTruncated || sourcePages > pagesToProcess,
                    )
                }
            } finally {
                renderer.close()
            }
        } finally {
            descriptor.close()
        }
    }

    internal fun boundedRenderSize(sourceWidth: Int, sourceHeight: Int): Pair<Int, Int> {
        require(sourceWidth > 0 && sourceHeight > 0)
        val dimensionScale = minOf(
            MAX_RENDER_DIMENSION.toDouble() / sourceWidth.toDouble(),
            MAX_RENDER_DIMENSION.toDouble() / sourceHeight.toDouble(),
        )
        val pixelScale = sqrt(
            MAX_RENDER_PIXELS.toDouble() /
                (sourceWidth.toDouble() * sourceHeight.toDouble()),
        )
        val scale = minOf(MAX_RENDER_SCALE, dimensionScale, pixelScale).coerceAtLeast(0.01)
        return maxOf(1, (sourceWidth * scale).roundToInt()) to
            maxOf(1, (sourceHeight * scale).roundToInt())
    }

    /** Returns true when the character limit was reached. */
    internal fun appendBounded(output: StringBuilder, rawText: String, pageNumber: Int): Boolean {
        val text = rawText.trim()
        if (text.isEmpty()) return false
        val prefix = if (output.isEmpty()) "" else "\n\n[Page $pageNumber]\n"
        val candidate = prefix + text
        val remaining = MAX_OCR_TEXT_CHARS - output.length
        if (remaining <= 0) return true
        if (candidate.length <= remaining) {
            output.append(candidate)
            return false
        }
        output.append(candidate, 0, remaining)
        return true
    }

    private inline fun <T> withRecognizer(block: (TextRecognizer) -> T): T {
        val recognizer = TextRecognition.getClient(
            DevanagariTextRecognizerOptions.Builder().build(),
        )
        return try {
            block(recognizer)
        } finally {
            recognizer.close()
        }
    }

    private fun textResult(
        text: String,
        pagesProcessed: Int,
        sourcePages: Int,
        truncated: Boolean,
    ): JSONObject {
        val boundedText = if (text.length <= MAX_OCR_TEXT_CHARS) {
            text
        } else {
            text.take(MAX_OCR_TEXT_CHARS)
        }
        return baseResult(if (boundedText.isBlank()) "empty" else "succeeded")
            .put("text", boundedText)
            .put("characters", boundedText.length)
            .put("pages_processed", pagesProcessed)
            .put("source_pages", sourcePages)
            .put("truncated", truncated || text.length > boundedText.length)
    }

    private fun failedResult(code: String): JSONObject = baseResult("failed")
        .put("error_code", code)

    private fun baseResult(status: String): JSONObject = JSONObject()
        .put("status", status)
        .put("engine", OCR_ENGINE)

    private const val OCR_ENGINE = "mlkit_text_recognition_v2_latin_devanagari"
    private const val PDF_MIME_TYPE = "application/pdf"
    private const val OCR_TASK_TIMEOUT_SECONDS = 8L
    private const val MAX_PDF_PAGES = 8
    private const val MAX_OCR_TEXT_CHARS = 64 * 1024
    private const val MAX_RENDER_DIMENSION = 2400
    private const val MAX_RENDER_PIXELS = 4_000_000
    private const val MAX_RENDER_SCALE = 2.0
}
