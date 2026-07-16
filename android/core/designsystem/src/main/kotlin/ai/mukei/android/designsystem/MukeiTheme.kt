package ai.mukei.android.designsystem

import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.graphics.Color

object MukeiColors {
    val Espresso = Color(0xFF2B211A)
    val Copper = Color(0xFFB87333)
    val Paper = Color(0xFFF1E8DC)
    val InkMuted = Color(0xFF6E6258)
    val DarkSurface = Color(0xFF1C1713)
    val DarkPaper = Color(0xFFE9DED1)
}

private val LightColors = lightColorScheme(
    primary = MukeiColors.Copper,
    onPrimary = Color.White,
    primaryContainer = Color(0xFFF4D7BE),
    onPrimaryContainer = MukeiColors.Espresso,
    background = MukeiColors.Paper,
    onBackground = MukeiColors.Espresso,
    surface = MukeiColors.Paper,
    onSurface = MukeiColors.Espresso,
    surfaceVariant = Color(0xFFE5D9CC),
    onSurfaceVariant = MukeiColors.InkMuted,
)

private val DarkColors = darkColorScheme(
    primary = Color(0xFFE1A06C),
    onPrimary = Color(0xFF3D1E08),
    primaryContainer = Color(0xFF6A3C1B),
    onPrimaryContainer = Color(0xFFFFDBC2),
    background = MukeiColors.DarkSurface,
    onBackground = MukeiColors.DarkPaper,
    surface = MukeiColors.DarkSurface,
    onSurface = MukeiColors.DarkPaper,
    surfaceVariant = Color(0xFF4A4038),
    onSurfaceVariant = Color(0xFFD4C4B7),
)

@Composable
fun MukeiTheme(
    darkTheme: Boolean = isSystemInDarkTheme(),
    content: @Composable () -> Unit,
) {
    MaterialTheme(
        colorScheme = if (darkTheme) DarkColors else LightColors,
        content = content,
    )
}
