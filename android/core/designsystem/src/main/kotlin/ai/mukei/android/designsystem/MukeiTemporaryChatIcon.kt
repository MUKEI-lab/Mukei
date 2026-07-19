package ai.mukei.android.designsystem

import androidx.compose.animation.animateColorAsState
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.tween
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.layout.size
import androidx.compose.material3.LocalContentColor
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
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
 * Incognito-style glyph for a process-local Temporary Chat session.
 *
 * The caller owns the actual privacy/session state. This glyph only reflects that state visually:
 * - normal/temporary tint changes morph smoothly;
 * - active Temporary Chat gets a restrained filled-lens treatment;
 * - an immediately-visible transition ring appears while the owning IconButton is disabled during
 *   the native begin/end hand-off.
 *
 * The visual feedback may be optimistic, but session truth remains owned by the runtime contract.
 */
@Composable
fun MukeiTemporaryChatIcon(
    contentDescription: String?,
    modifier: Modifier = Modifier,
    tint: Color = LocalContentColor.current,
    size: Dp = 22.dp,
) {
    val ambientContent = LocalContentColor.current
    val active = tint == MaterialTheme.colorScheme.primary

    // Material3 IconButton lowers LocalContentColor alpha while disabled. The Temporary Chat
    // action is disabled only during begin/end hand-off, so this gives immediate transition
    // feedback without duplicating runtime state inside the design-system component.
    val transitioning = ambientContent.alpha < 0.99f
    val animatedTint by animateColorAsState(
        targetValue = tint,
        animationSpec = tween(durationMillis = 180),
        label = "temporary-chat-tint",
    )
    val activeProgress by animateFloatAsState(
        targetValue = if (active) 1f else 0f,
        animationSpec = tween(durationMillis = 190),
        label = "temporary-chat-active",
    )
    val transitionProgress by animateFloatAsState(
        targetValue = if (transitioning) 1f else 0f,
        animationSpec = tween(durationMillis = 140),
        label = "temporary-chat-transition",
    )

    val describedModifier = if (contentDescription == null) {
        modifier
    } else {
        modifier.semantics { this.contentDescription = contentDescription }
    }
    val density = LocalDensity.current

    Canvas(modifier = describedModifier.size(size)) {
        val baseStrokeWidth = with(density) { 1.7.dp.toPx() }
        val strokeWidth = baseStrokeWidth * (1f + 0.08f * transitionProgress)
        val stroke = Stroke(
            width = strokeWidth,
            cap = StrokeCap.Round,
            join = StrokeJoin.Round,
        )
        val w = this.size.width
        val h = this.size.height
        fun p(x: Float, y: Float) = Offset(w * x, h * y)

        // During native mode hand-off, a restrained partial ring appears immediately. It makes
        // the tap feel acknowledged without pretending that the backend switch has completed.
        if (transitionProgress > 0f) {
            drawArc(
                color = animatedTint.copy(alpha = 0.34f * transitionProgress),
                startAngle = -58f,
                sweepAngle = 244f,
                useCenter = false,
                topLeft = p(0.08f, 0.08f),
                size = Size(w * 0.84f, h * 0.84f),
                style = Stroke(
                    width = baseStrokeWidth * 0.78f,
                    cap = StrokeCap.Round,
                ),
            )
        }

        val hatLift = 0.025f * transitionProgress + 0.008f * activeProgress
        val glassesDrop = 0.018f * transitionProgress

        // Minimal hat/brim silhouette.
        val hat = Path().apply {
            moveTo(w * 0.30f, h * (0.39f - hatLift))
            lineTo(w * 0.39f, h * (0.19f - hatLift))
            lineTo(w * 0.61f, h * (0.19f - hatLift))
            lineTo(w * 0.70f, h * (0.39f - hatLift))
        }
        drawPath(hat, animatedTint, style = stroke)
        drawLine(
            animatedTint,
            p(0.18f, 0.42f - hatLift),
            p(0.82f, 0.42f - hatLift),
            strokeWidth,
            StrokeCap.Round,
        )

        val leftLensCenter = p(0.34f, 0.64f + glassesDrop)
        val rightLensCenter = p(0.66f, 0.64f + glassesDrop)
        val lensRadius = w * 0.145f

        // Active Temporary Chat settles into a subtle filled-lens state. This remains restrained
        // enough for the top bar while making mode state visible without relying on color alone.
        if (activeProgress > 0f) {
            val fill = animatedTint.copy(alpha = 0.11f * activeProgress)
            drawCircle(fill, radius = lensRadius * 0.82f, center = leftLensCenter)
            drawCircle(fill, radius = lensRadius * 0.82f, center = rightLensCenter)
            drawCircle(
                animatedTint.copy(alpha = 0.55f * activeProgress),
                radius = w * 0.025f * activeProgress,
                center = p(0.50f, 0.83f),
            )
        }

        // Incognito glasses: visually distinct from a chat bubble and from New Chat.
        drawCircle(animatedTint, radius = lensRadius, center = leftLensCenter, style = stroke)
        drawCircle(animatedTint, radius = lensRadius, center = rightLensCenter, style = stroke)
        drawLine(
            animatedTint,
            p(0.485f, 0.64f + glassesDrop),
            p(0.515f, 0.64f + glassesDrop),
            strokeWidth,
            StrokeCap.Round,
        )
        drawLine(
            animatedTint,
            p(0.18f, 0.58f + glassesDrop),
            p(0.20f, 0.58f + glassesDrop),
            strokeWidth,
            StrokeCap.Round,
        )
        drawLine(
            animatedTint,
            p(0.80f, 0.58f + glassesDrop),
            p(0.82f, 0.58f + glassesDrop),
            strokeWidth,
            StrokeCap.Round,
        )
    }
}
