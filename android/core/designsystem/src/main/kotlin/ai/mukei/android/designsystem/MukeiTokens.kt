package ai.mukei.android.designsystem

import androidx.compose.ui.unit.dp

/** Semantic spacing scale from Mukei UI/UX Blueprint v0.1. */
object MukeiSpacing {
    val Micro = 4.dp
    val ExtraSmall = 8.dp
    val Small = 12.dp
    val Medium = 16.dp
    val Comfortable = 20.dp
    val Large = 24.dp
    val Section = 32.dp
    val LargeSection = 40.dp
    val Major = 56.dp
    val OpeningBreath = 72.dp
}

/** Corner-radius scale from Mukei UI/UX Blueprint v0.1. */
object MukeiRadius {
    val Small = 8.dp
    val Chip = 12.dp
    val Card = 16.dp
    val LargeCard = 20.dp
    val Sheet = 24.dp
    val Composer = 28.dp
}

/** Motion timing contract. Feature animation code should reference these values. */
object MukeiMotion {
    const val FastMilliseconds = 150
    const val StandardMinMilliseconds = 180
    const val StandardMaxMilliseconds = 220
    const val ComplexMaxMilliseconds = 250

    val TranslateMin = 8.dp
    val TranslateMax = 12.dp
}

object MukeiLayout {
    val PhoneTextPaddingMin = 16.dp
    val PhoneTextPaddingComfortable = 20.dp
    val LargePhoneTextPadding = 24.dp
    val MinimumTouchTarget = 48.dp
    val ReadableContentMaxWidth = 720.dp
    val ReadinessProgressMaxWidth = 280.dp
    val ComposerTextMinHeight = 104.dp
    val CompactStatusIconContainer = 40.dp

    const val DrawerWidthFraction = 0.86f
    const val TargetLineLengthCharactersMin = 60
    const val TargetLineLengthCharactersMax = 75
}
