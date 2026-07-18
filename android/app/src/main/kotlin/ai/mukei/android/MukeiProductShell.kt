package ai.mukei.android

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.DrawerValue
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FilterChip
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.IconButton
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
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import ai.mukei.android.designsystem.MukeiLayout
import ai.mukei.android.designsystem.MukeiMark
import ai.mukei.android.designsystem.MukeiSpacing

enum class TopLevelDestination(
    val drawerLabel: String,
    val screenTitle: String,
) {
    HOME("Mukei", ""),
    STORAGE("Storage", "Storage"),
    PROJECTS("Projects", "Projects"),
    MODELS("Models", "Models"),
    CHATS("Chats", "Chats"),
    SETTINGS("Settings", "Settings"),
}

private data class HomeCapability(
    val id: String,
    val label: String,
    val placeholder: String,
)

private val HomeCapabilities = listOf(
    HomeCapability("research", "Deep Research", "What should Mukei research?"),
    HomeCapability("build", "Build App", "Describe the app you want made…"),
    HomeCapability("files", "Read Files", "What should Mukei do with your files?"),
    HomeCapability("write", "Write", "What should we write?"),
    HomeCapability("code", "Code", "Describe what you want to build or fix…"),
)

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
                .padding(MukeiSpacing.Section),
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.Center,
        ) {
            MukeiMark(contentDescription = null)
            Spacer(Modifier.height(MukeiSpacing.Large))
            Text(
                text = "Opening Mukei…",
                style = MaterialTheme.typography.headlineSmall,
            )
            Spacer(Modifier.height(MukeiSpacing.Comfortable))
            LinearProgressIndicator(
                modifier = Modifier.widthIn(max = MukeiLayout.ReadinessProgressMaxWidth),
            )
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
                .verticalScroll(rememberScrollState())
                .padding(MukeiSpacing.Section),
            verticalArrangement = Arrangement.Center,
        ) {
            Text(
                text = "Mukei couldn’t start securely.",
                style = MaterialTheme.typography.headlineSmall,
            )
            Spacer(Modifier.height(MukeiSpacing.Small))
            Text(
                text = "Your local data has not been opened for normal use. Close and reopen the app while this internal build has no safe in-process retry path.",
                style = MaterialTheme.typography.bodyLarge,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Spacer(Modifier.height(MukeiSpacing.Medium))
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
    var newChatGeneration by rememberSaveable { mutableIntStateOf(0) }

    LaunchedEffect(openDrawerRequest) {
        if (openDrawerRequest > 0) drawerState.open()
    }
    LaunchedEffect(closeDrawerRequest) {
        if (closeDrawerRequest > 0) drawerState.close()
    }

    BackHandler(enabled = drawerState.isOpen || selected != TopLevelDestination.HOME) {
        if (drawerState.isOpen) {
            closeDrawerRequest += 1
        } else {
            selectedName = TopLevelDestination.HOME.name
        }
    }

    ModalNavigationDrawer(
        drawerState = drawerState,
        drawerContent = {
            ModalDrawerSheet(
                modifier = Modifier.fillMaxWidth(MukeiLayout.DrawerWidthFraction),
            ) {
                Column(
                    modifier = Modifier
                        .fillMaxHeight()
                        .padding(
                            horizontal = MukeiSpacing.Small,
                            vertical = MukeiSpacing.Comfortable,
                        ),
                    verticalArrangement = Arrangement.spacedBy(MukeiSpacing.Micro),
                ) {
                    DrawerDestinationItem(
                        destination = TopLevelDestination.HOME,
                        selected = selected,
                        onSelect = {
                            selectedName = TopLevelDestination.HOME.name
                            closeDrawerRequest += 1
                        },
                    )
                    HorizontalDivider(modifier = Modifier.padding(vertical = MukeiSpacing.ExtraSmall))
                    listOf(
                        TopLevelDestination.STORAGE,
                        TopLevelDestination.PROJECTS,
                        TopLevelDestination.MODELS,
                    ).forEach { destination ->
                        DrawerDestinationItem(
                            destination = destination,
                            selected = selected,
                            onSelect = {
                                selectedName = destination.name
                                closeDrawerRequest += 1
                            },
                        )
                    }
                    Spacer(Modifier.height(MukeiSpacing.Small))
                    DrawerDestinationItem(
                        destination = TopLevelDestination.CHATS,
                        selected = selected,
                        onSelect = {
                            selectedName = TopLevelDestination.CHATS.name
                            closeDrawerRequest += 1
                        },
                    )
                    Spacer(Modifier.weight(1f))
                    HorizontalDivider(modifier = Modifier.padding(vertical = MukeiSpacing.ExtraSmall))
                    DrawerDestinationItem(
                        destination = TopLevelDestination.SETTINGS,
                        selected = selected,
                        onSelect = {
                            selectedName = TopLevelDestination.SETTINGS.name
                            closeDrawerRequest += 1
                        },
                    )
                }
            }
        },
    ) {
        Scaffold(
            topBar = {
                TopAppBar(
                    title = {
                        if (selected.screenTitle.isNotEmpty()) {
                            Text(selected.screenTitle)
                        }
                    },
                    navigationIcon = {
                        IconButton(
                            onClick = { openDrawerRequest += 1 },
                            modifier = Modifier.semantics {
                                contentDescription = "Open navigation"
                            },
                        ) {
                            Text("☰", style = MaterialTheme.typography.titleLarge)
                        }
                    },
                    actions = {
                        IconButton(
                            onClick = {
                                selectedName = TopLevelDestination.HOME.name
                                newChatGeneration += 1
                            },
                            modifier = Modifier.semantics {
                                contentDescription = "New chat"
                            },
                        ) {
                            Text("＋", style = MaterialTheme.typography.titleLarge)
                        }
                        IconButton(
                            onClick = {},
                            enabled = false,
                            modifier = Modifier.semantics {
                                contentDescription = "Options unavailable in this build"
                            },
                        ) {
                            Text("⋮", style = MaterialTheme.typography.titleLarge)
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
                        resetGeneration = newChatGeneration,
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
private fun DrawerDestinationItem(
    destination: TopLevelDestination,
    selected: TopLevelDestination,
    onSelect: () -> Unit,
) {
    NavigationDrawerItem(
        label = {
            Text(
                text = destination.drawerLabel,
                fontWeight = if (selected == destination) FontWeight.Medium else FontWeight.Normal,
            )
        },
        selected = selected == destination,
        onClick = onSelect,
    )
}

@Composable
private fun HomeSurface(
    readiness: AppReadiness,
    resetGeneration: Int,
    openModels: () -> Unit,
) {
    var draft by rememberSaveable { mutableStateOf("") }
    var selectedCapabilityId by rememberSaveable { mutableStateOf<String?>(null) }
    val selectedCapability = HomeCapabilities.firstOrNull { it.id == selectedCapabilityId }

    LaunchedEffect(resetGeneration) {
        if (resetGeneration > 0) {
            draft = ""
            selectedCapabilityId = null
        }
    }

    Box(
        modifier = Modifier
            .fillMaxSize()
            .padding(horizontal = MukeiLayout.LargePhoneTextPadding),
    ) {
        Column(
            modifier = Modifier
                .align(Alignment.Center)
                .fillMaxWidth()
                .widthIn(max = MukeiLayout.ReadableContentMaxWidth)
                .verticalScroll(rememberScrollState())
                .padding(vertical = MukeiSpacing.Large),
            horizontalAlignment = Alignment.CenterHorizontally,
        ) {
            Text(
                text = "What’s on your mind?",
                style = MaterialTheme.typography.headlineLarge,
            )
            Spacer(Modifier.height(MukeiSpacing.Small))
            Text(
                text = "Start naturally. Ask, build, research, write, or describe what you want made.",
                style = MaterialTheme.typography.bodyLarge,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Spacer(Modifier.height(MukeiSpacing.Section))

            if (readiness.inference.status == ReadinessStatus.ACTION_REQUIRED) {
                Surface(
                    modifier = Modifier.fillMaxWidth(),
                    shape = MaterialTheme.shapes.large,
                    color = MaterialTheme.colorScheme.surfaceVariant,
                ) {
                    Column(
                        modifier = Modifier.padding(MukeiSpacing.Comfortable),
                        verticalArrangement = Arrangement.spacedBy(MukeiSpacing.ExtraSmall),
                    ) {
                        Text(
                            text = "Model artifacts required",
                            style = MaterialTheme.typography.titleMedium,
                            fontWeight = FontWeight.Medium,
                        )
                        Text(
                            text = "The secure backend and encrypted storage are ready. Conversation inference still needs a model.",
                            style = MaterialTheme.typography.bodyMedium,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                        TextButton(onClick = openModels) {
                            Text("Open Models")
                        }
                    }
                }
                Spacer(Modifier.height(MukeiSpacing.Comfortable))
            }

            OutlinedTextField(
                value = draft,
                onValueChange = { draft = it },
                modifier = Modifier.fillMaxWidth(),
                placeholder = {
                    Text(selectedCapability?.placeholder ?: "Tell Mukei what you want to do…")
                },
                supportingText = {
                    Text("Sending is intentionally disabled until the Conversation vertical slice is wired end-to-end.")
                },
                minLines = 3,
            )
            Spacer(Modifier.height(MukeiSpacing.Small))
            Row(
                modifier = Modifier
                    .fillMaxWidth()
                    .horizontalScroll(rememberScrollState()),
                horizontalArrangement = Arrangement.spacedBy(MukeiSpacing.ExtraSmall),
            ) {
                HomeCapabilities.forEach { capability ->
                    FilterChip(
                        selected = selectedCapabilityId == capability.id,
                        onClick = {
                            selectedCapabilityId = if (selectedCapabilityId == capability.id) {
                                null
                            } else {
                                capability.id
                            }
                        },
                        label = { Text(capability.label) },
                    )
                }
            }
            Spacer(Modifier.height(MukeiSpacing.Small))
            Button(
                onClick = {},
                enabled = false,
                modifier = Modifier.align(Alignment.End),
            ) {
                Text("Send")
            }
            Spacer(Modifier.height(MukeiSpacing.Section))
            Text(
                text = "Private intelligence · local-first foundation",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
    }
}

@Composable
private fun ModelsSurface(readiness: AppReadiness) {
    Column(
        modifier = Modifier
            .fillMaxSize()
            .verticalScroll(rememberScrollState())
            .padding(MukeiLayout.LargePhoneTextPadding),
        verticalArrangement = Arrangement.spacedBy(MukeiSpacing.Small),
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
            .verticalScroll(rememberScrollState())
            .padding(MukeiLayout.LargePhoneTextPadding),
        verticalArrangement = Arrangement.spacedBy(MukeiSpacing.Small),
    ) {
        Text(
            text = destination.screenTitle,
            style = MaterialTheme.typography.headlineMedium,
        )
        Text(
            text = "This destination exists in the product shell, but its feature data and actions are intentionally not exposed until that vertical slice is implemented.",
            style = MaterialTheme.typography.bodyLarge,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }
}
