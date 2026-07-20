package ai.mukei.android

import androidx.compose.runtime.Composable

@Composable
internal fun MukeiAppShell(backendState: BackendRuntimeHost.State) {
    MukeiProductShell(backendState)
}
