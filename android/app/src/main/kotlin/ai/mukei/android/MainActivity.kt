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
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import ai.mukei.android.protocol.ProtocolVersion

class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContent {
            MaterialTheme {
                MukeiApp()
            }
        }
    }
}

@Composable
private fun MukeiApp() {
    Surface(modifier = Modifier.fillMaxSize()) {
        Column(
            modifier = Modifier
                .fillMaxSize()
                .padding(24.dp),
            verticalArrangement = Arrangement.spacedBy(16.dp),
        ) {
            Text(
                text = "Mukei",
                style = MaterialTheme.typography.headlineLarge,
            )
            Text(
                text = "Kotlin Android production scaffold",
                style = MaterialTheme.typography.titleMedium,
            )
            LinearProgressIndicator()
            Text(
                text = "Protocol ${ProtocolVersion.CURRENT.major}.${ProtocolVersion.CURRENT.minor} · native runtime pending",
                style = MaterialTheme.typography.bodyMedium,
            )
        }
    }
}
