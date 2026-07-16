package ai.mukei.android

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import ai.mukei.android.designsystem.MukeiMark
import ai.mukei.android.designsystem.MukeiTheme
import ai.mukei.android.protocol.ProtocolVersion

class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContent {
            MukeiTheme {
                MukeiApp(BackendRuntimeHost.state)
            }
        }
    }
}

@Composable
private fun MukeiApp(backendState: BackendRuntimeHost.State) {
    Surface(
        modifier = Modifier.fillMaxSize(),
        color = MaterialTheme.colorScheme.background,
    ) {
        Column(
            modifier = Modifier
                .fillMaxSize()
                .padding(horizontal = 24.dp, vertical = 40.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.spacedBy(18.dp),
        ) {
            MukeiMark()
            Text(
                text = "Mukei",
                style = MaterialTheme.typography.headlineLarge,
                color = MaterialTheme.colorScheme.onBackground,
            )
            Text(
                text = "Private intelligence. On device.",
                style = MaterialTheme.typography.titleMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            if (backendState is BackendRuntimeHost.State.Starting) {
                LinearProgressIndicator()
            }
            Text(
                text = when (backendState) {
                    BackendRuntimeHost.State.Starting -> "Starting encrypted native runtime"
                    is BackendRuntimeHost.State.Ready -> "Backend ready · ${backendState.securitySummary}"
                    is BackendRuntimeHost.State.Failed -> "Backend unavailable · ${backendState.code}"
                    BackendRuntimeHost.State.Stopped -> "Backend stopped"
                },
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Text(
                text = "Protocol ${ProtocolVersion.CURRENT.major}.${ProtocolVersion.CURRENT.minor}",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
    }
}
