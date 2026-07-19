package ai.mukei.android.designsystem

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.layout.size
import androidx.compose.material3.LocalContentColor
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
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
 * Incognito-style glyph for a process-local Temporary Chat session.
 *
 * It intentionally avoids looking like the generic overflow menu or the compose/new-chat action.
 */
@Composable
fun MukeiTemporaryChatIcon(
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

        // Minimal hat/brim silhouette.
        val hat = Path().apply {
            moveTo(w * 0.30f, h * 0.39f)
            lineTo(w * 0.39f, h * 0.19f)
            lineTo(w * 0.61f, h * 0.19f)
            lineTo(w * 0.70f, h * 0.39f)
        }
        drawPath(hat, tint, style = stroke)
        drawLine(tint, p(0.18f, 0.42f), p(0.82f, 0.42f), strokeWidth, StrokeCap.Round)

        // Incognito glasses: visually distinct from a chat bubble and from New Chat.
        drawCircle(tint, radius = w * 0.145f, center = p(0.34f, 0.64f), style = stroke)
        drawCircle(tint, radius = w * 0.145f, center = p(0.66f, 0.64f), style = stroke)
        drawLine(tint, p(0.485f, 0.64f), p(0.515f, 0.64f), strokeWidth, StrokeCap.Round)
        drawLine(tint, p(0.18f, 0.58f), p(0.20f, 0.58f), strokeWidth, StrokeCap.Round)
        drawLine(tint, p(0.80f, 0.58f), p(0.82f, 0.58f), strokeWidth, StrokeCap.Round)
    }
}
