package ai.mukei.android

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.weight
import androidx.compose.foundation.layout.widthIn
import androidx.compose.material3.Button
import androidx.compose.material3.DrawerValue
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalDrawerSheet
import androidx.compose.material3.ModalNavigationDrawer
import androidx.compose.material3.NavigationDrawerItem
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.rememberDrawerState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import ai.mukei.android.designsystem.MukeiMark

enum class TopLevelDestination(
    val label: String,
) {
    HOME("Home"),
    CHATS("Chats"),
    STORAGE("Storage"),
    PROJECTS("Projects"),
    MODELS("Models"),
    SETTINGS("Settings"),
}

@Composable
fun MukeiProductShell(
    backendState: BackendRuntimeHost.State,
) {
    when (backendState) {
        BackendRuntimeHost.State.Starting -> StartupSurface()
        is BackendRuntimeHost.State.Failed -> StartupFailureSurface(backendState.code)
        BackendRuntimeHost.State.Stopped -> StartupFailureSurface("backend_stopped")
        is BackendRuntimeHost.State.Ready -> ReadyProductShell(backendState)
    }
}

@Composable
private fun StartupSurface() {
    Surface(
        modifier = Modifier.fillMaxSize(),
        color = MaterialTheme.colorScheme.background,
    ) {
        Column(
            modifier = Modifier
                .fillMaxSize()
                .padding(32.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.Center,
        ) {
            MukeiMark(contentDescription = null)
            Spacer(Modifier.height(24.dp))
            Text(
                text = "Opening Mukei…",
                style = MaterialTheme.typography.headlineSmall,
            )
            Spacer(Modifier.height(20.dp))
            LinearProgressIndicator(modifier = Modifier.widthIn(max = 280.dp))
        }
    }
}

@Composable
private fun StartupFailureSurface(code: String) {
    Surface(
        modifier = Modifier.fillMaxSize(),
        color = MaterialTheme.colorScheme.background,
    ) {
        Column(
            modifier = Modifier
                .fillMaxSize()
                .padding(32.dp),
            verticalArrangement = Arrangement.Center,
        ) {
            Text(
                text = "Mukei couldn’t start securely.",
                style = MaterialTheme.typography.headlineSmall,
            )
            Spacer(Modifier.height(12.dp))
            Text(
                text = "Your local data has not been opened for normal use. Close and reopen the app while this internal build has no safe in-process retry path.",
                style = MaterialTheme.typography.bodyLarge,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Spacer(Modifier.height(16.dp))
            Text(
                text = "Diagnostic code: $code",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun ReadyProductShell(state: BackendRuntimeHost.State.Ready) {
    var selectedName by rememberSaveable { mutableStateOf(TopLevelDestination.HOME.name) }
    val selected = TopLevelDestination.valueOf(selectedName)
    val drawerState = rememberDrawerState(initialValue = DrawerValue.Closed)
    var openDrawerRequest by remember { mutableIntStateOf(0) }
    var closeDrawerRequest by remember { mutableIntStateOf(0) }

    LaunchedEffect(openDrawerRequest) {
        if (openDrawerRequest > 0) drawerState.open()
    }
    LaunchedEffect(closeDrawerRequest) {
        if (closeDrawerRequest > 0) drawerState.close()
    }

    ModalNavigationDrawer(
        drawerState = drawerState,
        drawerContent = {
            ModalDrawerSheet {
                Column(
                    modifier = Modifier.padding(horizontal = 12.dp, vertical = 20.dp),
                    verticalArrangement = Arrangement.spacedBy(4.dp),
                ) {
                    Text(
                        text = "Mukei",
                        modifier = Modifier.padding(horizontal = 16.dp, vertical = 12.dp),
                        style = MaterialTheme.typography.titleLarge,
                        fontWeight = FontWeight.Medium,
                    )
                    HorizontalDivider(modifier = Modifier.padding(bottom = 8.dp))
                    TopLevelDestination.entries.forEach { destination ->
                        NavigationDrawerItem(
                            label = { Text(destination.label) },
                            selected = selected == destination,
                            onClick = {
                                selectedName = destination.name
                                closeDrawerRequest += 1
                            },
                        )
                    }
                }
            }
        },
    ) {
        Scaffold(
            topBar = {
                TopAppBar(
                    title = { Text(selected.label) },
                    navigationIcon = {
                        TextButton(onClick = { openDrawerRequest += 1 }) {
                            Text("Menu")
                        }
                    },
                )
            },
        ) { innerPadding ->
            Box(
                modifier = Modifier
                    .fillMaxSize()
                    .padding(innerPadding),
            ) {
                when (selected) {
                    TopLevelDestination.HOME -> HomeSurface(
                        readiness = state.readiness,
                        openModels = { selectedName = TopLevelDestination.MODELS.name },
                    )
                    TopLevelDestination.MODELS -> ModelsSurface(state.readiness)
                    else -> ReservedDestinationSurface(selected)
                }
            }
        }
    }
}

@Composable
private fun HomeSurface(
    readiness: AppReadiness,
    openModels: () -> Unit,
) {
    var draft by rememberSaveable { mutableStateOf("") }

    Column(
        modifier = Modifier
            .fillMaxSize()
            .padding(horizontal = 24.dp, vertical = 28.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Spacer(Modifier.weight(1f))
        Column(
            modifier = Modifier
                .fillMaxWidth()
                .widthIn(max = 720.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
        ) {
            Text(
                text = "What’s on your mind?",
                style = MaterialTheme.typography.headlineLarge,
            )
            Spacer(Modifier.height(10.dp))
            Text(
                text = "Start naturally. Mukei keeps the product shell and encrypted local storage usable even when an inference model still needs to be installed.",
                style = MaterialTheme.typography.bodyLarge,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Spacer(Modifier.height(28.dp))

            if (readiness.inference.status == ReadinessStatus.ACTION_REQUIRED) {
                Surface(
                    modifier = Modifier.fillMaxWidth(),
                    shape = MaterialTheme.shapes.large,
                    color = MaterialTheme.colorScheme.surfaceVariant,
                ) {
                    Column(
                        modifier = Modifier.padding(18.dp),
                        verticalArrangement = Arrangement.spacedBy(8.dp),
                    ) {
                        Text(
                            text = "Model artifacts required",
                            style = MaterialTheme.typography.titleMedium,
                            fontWeight = FontWeight.Medium,
                        )
                        Text(
                            text = "The secure backend is ready. Conversation inference is the missing capability, not an app startup failure.",
                            style = MaterialTheme.typography.bodyMedium,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                        TextButton(onClick = openModels) {
                            Text("Open Models")
                        }
                    }
                }
                Spacer(Modifier.height(18.dp))
            }

            OutlinedTextField(
                value = draft,
                onValueChange = { draft = it },
                modifier = Modifier.fillMaxWidth(),
                placeholder = { Text("Message Mukei") },
                supportingText = {
                    Text("Sending is intentionally disabled until the Conversation vertical slice is wired end-to-end.")
                },
                minLines = 3,
            )
            Spacer(Modifier.height(12.dp))
            Button(
                onClick = {},
                enabled = false,
                modifier = Modifier.align(Alignment.End),
            ) {
                Text("Send")
            }
        }
        Spacer(Modifier.weight(1f))
        Text(
            text = "Private intelligence · local-first foundation",
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }
}

@Composable
private fun ModelsSurface(readiness: AppReadiness) {
    Column(
        modifier = Modifier
            .fillMaxSize()
            .padding(24.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        Text(
            text = "Models",
            style = MaterialTheme.typography.headlineMedium,
        )
        Text(
            text = when (readiness.inference.status) {
                ReadinessStatus.READY -> "Inference capability is ready."
                ReadinessStatus.ACTION_REQUIRED -> "Local inference artifacts are required before conversation can run. Model installation UI is scheduled for the Models vertical slice."
                ReadinessStatus.DEGRADED -> "Inference capability is degraded."
                ReadinessStatus.UNAVAILABLE -> "Inference capability is unavailable in this runtime."
                ReadinessStatus.UNKNOWN -> "Inference capability state is still unknown."
            },
            style = MaterialTheme.typography.bodyLarge,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }
}

@Composable
private fun ReservedDestinationSurface(destination: TopLevelDestination) {
    Column(
        modifier = Modifier
            .fillMaxSize()
            .padding(24.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        Text(
            text = destination.label,
            style = MaterialTheme.typography.headlineMedium,
        )
        Text(
            text = "This destination exists in the product shell, but its feature data and actions are intentionally not exposed until that vertical slice is implemented.",
            style = MaterialTheme.typography.bodyLarge,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }
}
