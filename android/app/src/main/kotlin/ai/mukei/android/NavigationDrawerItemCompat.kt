package ai.mukei.android

import androidx.compose.material3.NavigationDrawerItem
import androidx.compose.runtime.Composable
import androidx.compose.ui.graphics.Shape

/** Compatibility overload for the Material3 version pinned by this app. */
@Composable
internal fun NavigationDrawerItem(
    selected: Boolean,
    onClick: () -> Unit,
    icon: @Composable () -> Unit,
    label: @Composable () -> Unit,
    enabled: Boolean,
    shape: Shape,
) {
    NavigationDrawerItem(
        selected = selected,
        onClick = {
            if (enabled) onClick()
        },
        icon = icon,
        label = label,
        shape = shape,
    )
}
