package ai.mukei.android

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import ai.mukei.android.designsystem.MukeiTheme

// CI refresh marker; reverted in the next commit.
class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContent {
            MukeiTheme {
                MukeiProductShell(BackendRuntimeHost.state)
            }
        }
    }
}
