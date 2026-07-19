package ai.mukei.android.designsystem

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.layout.size
import androidx.compose.material3.LocalContentColor
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

/**
 * New-conversation glyph: a rounded note surface with a diagonal compose pencil.
 *
 * The silhouette intentionally reads as “compose new” rather than edit-only,
 * external-link, or generic add. Kept as a dedicated design-system primitive so
 * the final vector asset can replace it without touching feature code.
 */
@Composable
fun MukeiNewChatIcon(
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

        // Rounded note/canvas. The upper-right break leaves room for the pencil.
        val notePath = Path().apply {
            moveTo(w * 0.70f, h * 0.18f)
            lineTo(w * 0.31f, h * 0.18f)
            quadraticBezierTo(w * 0.17f, h * 0.18f, w * 0.17f, h * 0.32f)
            lineTo(w * 0.17f, h * 0.72f)
            quadraticBezierTo(w * 0.17f, h * 0.84f, w * 0.31f, h * 0.84f)
            lineTo(w * 0.70f, h * 0.84f)
            quadraticBezierTo(w * 0.83f, h * 0.84f, w * 0.83f, h * 0.71f)
            lineTo(w * 0.83f, h * 0.55f)
        }
        drawPath(notePath, tint, style = stroke)

        // Diagonal pencil body.
        val pencil = Path().apply {
            moveTo(w * 0.43f, h * 0.58f)
            lineTo(w * 0.69f, h * 0.32f)
            lineTo(w * 0.78f, h * 0.41f)
            lineTo(w * 0.52f, h * 0.67f)
            close()
        }
        drawPath(pencil, tint, style = stroke)

        // Pencil tip and cap cues keep the glyph legible at 22dp.
        drawLine(
            color = tint,
            start = p(0.43f, 0.58f),
            end = p(0.40f, 0.70f),
            strokeWidth = strokeWidth,
            cap = StrokeCap.Round,
        )
        drawLine(
            color = tint,
            start = p(0.40f, 0.70f),
            end = p(0.52f, 0.67f),
            strokeWidth = strokeWidth,
            cap = StrokeCap.Round,
        )
        drawLine(
            color = tint,
            start = p(0.69f, 0.32f),
            end = p(0.73f, 0.28f),
            strokeWidth = strokeWidth,
            cap = StrokeCap.Round,
        )
        drawLine(
            color = tint,
            start = p(0.78f, 0.41f),
            end = p(0.82f, 0.37f),
            strokeWidth = strokeWidth,
            cap = StrokeCap.Round,
        )
        drawLine(
            color = tint,
            start = p(0.73f, 0.28f),
            end = p(0.82f, 0.37f),
            strokeWidth = strokeWidth,
            cap = StrokeCap.Round,
        )
    }
}
