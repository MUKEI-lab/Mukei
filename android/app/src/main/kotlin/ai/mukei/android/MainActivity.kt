package ai.mukei.android

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import ai.mukei.android.designsystem.MukeiTheme

class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContent {
            MukeiTheme {
                val backendState = BackendRuntimeHost.state
                if (backendState is BackendRuntimeHost.State.Ready) {
                    MukeiReadyProductShell(backendState)
                } else {
                    MukeiProductShell(backendState)
                }
            }
        }
    }
}
