package ai.mukei.android.designsystem

import androidx.compose.foundation.Canvas
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.StrokeJoin
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.foundation.layout.size
import androidx.compose.material3.LocalContentColor

/**
 * Small centralized icon language for the Android shell.
 *
 * These are intentionally simple thin-line glyphs so feature code does not hard-code
 * Unicode symbols or bind itself to a third-party icon package. The implementation can
 * later be swapped to approved Phosphor/vector assets without changing screen code.
 */
enum class MukeiIconKey {
    MENU,
    NEW_CHAT,
    MORE,
    HOME,
    STORAGE,
    PROJECTS,
    MODELS,
    CHATS,
    SETTINGS,
    ATTACH,
    SEND,
}

@Composable
fun MukeiIcon(
    icon: MukeiIconKey,
    contentDescription: String?,
    modifier: Modifier = Modifier,
    tint: Color = LocalContentColor.current,
    size: Dp = 22.dp,
) {
    val describedModifier = if (contentDescription == null) {
        modifier
    } else {
        modifier.semantics { this.contentDescription = contentDescription }
    }
    val density = LocalDensity.current
    Canvas(modifier = describedModifier.size(size)) {
        val strokeWidth = with(density) { 1.7.dp.toPx() }
        val stroke = Stroke(
            width = strokeWidth,
            cap = StrokeCap.Round,
            join = StrokeJoin.Round,
        )
        val w = this.size.width
        val h = this.size.height
        fun p(x: Float, y: Float) = Offset(w * x, h * y)

        when (icon) {
            MukeiIconKey.MENU -> {
                listOf(0.30f, 0.50f, 0.70f).forEach { y ->
                    drawLine(tint, p(0.18f, y), p(0.82f, y), strokeWidth, StrokeCap.Round)
                }
            }

            MukeiIconKey.NEW_CHAT -> {
                drawRoundRect(
                    color = tint,
                    topLeft = p(0.14f, 0.18f),
                    size = Size(w * 0.64f, h * 0.60f),
                    cornerRadius = CornerRadius(w * 0.13f, h * 0.13f),
                    style = stroke,
                )
                drawLine(tint, p(0.70f, 0.16f), p(0.70f, 0.42f), strokeWidth, StrokeCap.Round)
                drawLine(tint, p(0.57f, 0.29f), p(0.83f, 0.29f), strokeWidth, StrokeCap.Round)
            }

            MukeiIconKey.MORE -> {
                listOf(0.28f, 0.50f, 0.72f).forEach { y ->
                    drawCircle(tint, radius = w * 0.055f, center = p(0.50f, y))
                }
            }

            MukeiIconKey.HOME -> {
                val path = Path().apply {
                    moveTo(w * 0.16f, h * 0.48f)
                    lineTo(w * 0.50f, h * 0.19f)
                    lineTo(w * 0.84f, h * 0.48f)
                    moveTo(w * 0.23f, h * 0.43f)
                    lineTo(w * 0.23f, h * 0.80f)
                    lineTo(w * 0.77f, h * 0.80f)
                    lineTo(w * 0.77f, h * 0.43f)
                    moveTo(w * 0.43f, h * 0.80f)
                    lineTo(w * 0.43f, h * 0.61f)
                    lineTo(w * 0.57f, h * 0.61f)
                    lineTo(w * 0.57f, h * 0.80f)
                }
                drawPath(path, tint, style = stroke)
            }

            MukeiIconKey.STORAGE -> {
                drawRoundRect(
                    tint,
                    topLeft = p(0.18f, 0.22f),
                    size = Size(w * 0.64f, h * 0.23f),
                    cornerRadius = CornerRadius(w * 0.08f, h * 0.08f),
                    style = stroke,
                )
                drawRoundRect(
                    tint,
                    topLeft = p(0.18f, 0.55f),
                    size = Size(w * 0.64f, h * 0.23f),
                    cornerRadius = CornerRadius(w * 0.08f, h * 0.08f),
                    style = stroke,
                )
                drawCircle(tint, radius = w * 0.035f, center = p(0.70f, 0.335f))
                drawCircle(tint, radius = w * 0.035f, center = p(0.70f, 0.665f))
            }

            MukeiIconKey.PROJECTS -> {
                val path = Path().apply {
                    moveTo(w * 0.14f, h * 0.30f)
                    lineTo(w * 0.42f, h * 0.30f)
                    lineTo(w * 0.50f, h * 0.40f)
                    lineTo(w * 0.84f, h * 0.40f)
                    lineTo(w * 0.84f, h * 0.76f)
                    quadraticTo(w * 0.84f, h * 0.82f, w * 0.77f, h * 0.82f)
                    lineTo(w * 0.20f, h * 0.82f)
                    quadraticTo(w * 0.14f, h * 0.82f, w * 0.14f, h * 0.76f)
                    close()
                }
                drawPath(path, tint, style = stroke)
            }

            MukeiIconKey.MODELS -> {
                val path = Path().apply {
                    moveTo(w * 0.50f, h * 0.15f)
                    lineTo(w * 0.80f, h * 0.32f)
                    lineTo(w * 0.80f, h * 0.68f)
                    lineTo(w * 0.50f, h * 0.85f)
                    lineTo(w * 0.20f, h * 0.68f)
                    lineTo(w * 0.20f, h * 0.32f)
                    close()
                    moveTo(w * 0.20f, h * 0.32f)
                    lineTo(w * 0.50f, h * 0.50f)
                    lineTo(w * 0.80f, h * 0.32f)
                    moveTo(w * 0.50f, h * 0.50f)
                    lineTo(w * 0.50f, h * 0.85f)
                }
                drawPath(path, tint, style = stroke)
            }

            MukeiIconKey.CHATS -> {
                drawRoundRect(
                    tint,
                    topLeft = p(0.14f, 0.18f),
                    size = Size(w * 0.58f, h * 0.46f),
                    cornerRadius = CornerRadius(w * 0.13f, h * 0.13f),
                    style = stroke,
                )
                drawLine(tint, p(0.30f, 0.64f), p(0.23f, 0.76f), strokeWidth, StrokeCap.Round)
                drawRoundRect(
                    tint,
                    topLeft = p(0.35f, 0.42f),
                    size = Size(w * 0.51f, h * 0.37f),
                    cornerRadius = CornerRadius(w * 0.12f, h * 0.12f),
                    style = stroke,
                )
                drawLine(tint, p(0.70f, 0.79f), p(0.77f, 0.87f), strokeWidth, StrokeCap.Round)
            }

            MukeiIconKey.SETTINGS -> {
                drawCircle(tint, radius = w * 0.25f, center = p(0.50f, 0.50f), style = stroke)
                drawCircle(tint, radius = w * 0.08f, center = p(0.50f, 0.50f), style = stroke)
                listOf(
                    0.50f to 0.12f, 0.50f to 0.88f,
                    0.12f to 0.50f, 0.88f to 0.50f,
                    0.23f to 0.23f, 0.77f to 0.77f,
                    0.77f to 0.23f, 0.23f to 0.77f,
                ).forEach { (x, y) ->
                    val dx = x - 0.50f
                    val dy = y - 0.50f
                    val length = kotlin.math.sqrt(dx * dx + dy * dy)
                    val ux = dx / length
                    val uy = dy / length
                    drawLine(
                        tint,
                        p(0.50f + ux * 0.30f, 0.50f + uy * 0.30f),
                        p(0.50f + ux * 0.38f, 0.50f + uy * 0.38f),
                        strokeWidth,
                        StrokeCap.Round,
                    )
                }
            }

            MukeiIconKey.ATTACH -> {
                drawCircle(tint, radius = w * 0.30f, center = p(0.50f, 0.50f), style = stroke)
                drawLine(tint, p(0.50f, 0.34f), p(0.50f, 0.66f), strokeWidth, StrokeCap.Round)
                drawLine(tint, p(0.34f, 0.50f), p(0.66f, 0.50f), strokeWidth, StrokeCap.Round)
            }

            MukeiIconKey.SEND -> {
                drawLine(tint, p(0.20f, 0.50f), p(0.78f, 0.50f), strokeWidth, StrokeCap.Round)
                drawLine(tint, p(0.61f, 0.32f), p(0.79f, 0.50f), strokeWidth, StrokeCap.Round)
                drawLine(tint, p(0.61f, 0.68f), p(0.79f, 0.50f), strokeWidth, StrokeCap.Round)
            }
        }
    }
}
