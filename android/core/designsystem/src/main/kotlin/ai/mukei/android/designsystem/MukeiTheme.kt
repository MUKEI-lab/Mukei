package ai.mukei.android.designsystem

import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Shapes
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.graphics.Color

/** Semantic color seeds from Mukei UI/UX Blueprint v0.1. */
object MukeiColors {
    val Background = Color(0xFFF8F2EA)
    val Surface = Color(0xFFFFF9F1)
    val SurfaceMuted = Color(0xFFF2E8DC)
    val SurfaceElevated = Color(0xFFFFFDF8)
    val TextPrimary = Color(0xFF2B211A)
    val TextSecondary = Color(0xFF6B5A4A)
    val TextTertiary = Color(0xFF9A8D80)
    val Divider = Color(0xFFE7DCCF)
    val Accent = Color(0xFF8A6A4F)
    val AccentMuted = Color(0xFFE8D7C6)
    val Success = Color(0xFF687C5A)
    val SuccessMuted = Color(0xFFE4EBDD)
    val Warning = Color(0xFFA7793F)
    val WarningMuted = Color(0xFFF0E2C8)
    val Error = Color(0xFF9B5E55)
    val ErrorMuted = Color(0xFFF0DCD8)

    // Warm dark-theme seeds remain provisional until visual/device review.
    val DarkBackground = Color(0xFF1C1713)
    val DarkSurface = Color(0xFF241E19)
    val DarkSurfaceMuted = Color(0xFF3B322B)
    val DarkTextPrimary = Color(0xFFE9DED1)
    val DarkTextSecondary = Color(0xFFD4C4B7)
}

private val LightColors = lightColorScheme(
    primary = MukeiColors.Accent,
    onPrimary = MukeiColors.SurfaceElevated,
    primaryContainer = MukeiColors.AccentMuted,
    onPrimaryContainer = MukeiColors.TextPrimary,
    background = MukeiColors.Background,
    onBackground = MukeiColors.TextPrimary,
    surface = MukeiColors.Surface,
    onSurface = MukeiColors.TextPrimary,
    surfaceVariant = MukeiColors.SurfaceMuted,
    onSurfaceVariant = MukeiColors.TextSecondary,
    outline = MukeiColors.Divider,
    error = MukeiColors.Error,
    onError = MukeiColors.SurfaceElevated,
    errorContainer = MukeiColors.ErrorMuted,
    onErrorContainer = MukeiColors.TextPrimary,
)

private val DarkColors = darkColorScheme(
    primary = Color(0xFFD6B69C),
    onPrimary = Color(0xFF3B2A1E),
    primaryContainer = Color(0xFF5B4535),
    onPrimaryContainer = MukeiColors.DarkTextPrimary,
    background = MukeiColors.DarkBackground,
    onBackground = MukeiColors.DarkTextPrimary,
    surface = MukeiColors.DarkSurface,
    onSurface = MukeiColors.DarkTextPrimary,
    surfaceVariant = MukeiColors.DarkSurfaceMuted,
    onSurfaceVariant = MukeiColors.DarkTextSecondary,
    error = Color(0xFFE2AAA1),
    onError = Color(0xFF4B1712),
)

private val MukeiShapes = Shapes(
    extraSmall = RoundedCornerShape(MukeiRadius.Small),
    small = RoundedCornerShape(MukeiRadius.Chip),
    medium = RoundedCornerShape(MukeiRadius.Card),
    large = RoundedCornerShape(MukeiRadius.LargeCard),
    extraLarge = RoundedCornerShape(MukeiRadius.Sheet),
)

@Composable
fun MukeiTheme(
    darkTheme: Boolean = isSystemInDarkTheme(),
    content: @Composable () -> Unit,
) {
    MaterialTheme(
        colorScheme = if (darkTheme) DarkColors else LightColors,
        shapes = MukeiShapes,
        content = content,
    )
}
