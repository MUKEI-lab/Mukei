package ai.mukei.android

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.clickable
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
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.DrawerValue
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalDrawerSheet
import androidx.compose.material3.ModalNavigationDrawer
import androidx.compose.material3.NavigationDrawerItem
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
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
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import ai.mukei.android.designsystem.MukeiIcon
import ai.mukei.android.designsystem.MukeiIconKey
import ai.mukei.android.designsystem.MukeiLayout
import ai.mukei.android.designsystem.MukeiMark
import ai.mukei.android.designsystem.MukeiNewChatIcon
import ai.mukei.android.designsystem.MukeiRadius
import ai.mukei.android.designsystem.MukeiSpacing
import ai.mukei.android.designsystem.MukeiStroke
import java.time.LocalTime

enum class TopLevelDestination(
    val drawerLabel: String,
    val screenTitle: String,
    val icon: MukeiIconKey,
    val emptyTitle: String,
    val emptyBody: String,
) {
    HOME(
        drawerLabel = "Mukei",
        screenTitle = "",
        icon = MukeiIconKey.HOME,
        emptyTitle = "",
        emptyBody = "",
    ),
    STORAGE(
        drawerLabel = "Storage",
        screenTitle = "Storage",
        icon = MukeiIconKey.STORAGE,
        emptyTitle = "Your files will live here",
        emptyBody = "Imported files, generated work, and exports will appear here as the secure storage experience is connected.",
    ),
    PROJECTS(
        drawerLabel = "Projects",
        screenTitle = "Projects",
        icon = MukeiIconKey.PROJECTS,
        emptyTitle = "Keep long-running work together",
        emptyBody = "Projects will group related chats, files, workspaces, and artifacts without changing where your data is stored.",
    ),
    MODELS(
        drawerLabel = "Models",
        screenTitle = "Models",
        icon = MukeiIconKey.MODELS,
        emptyTitle = "",
        emptyBody = "",
    ),
    CHATS(
        drawerLabel = "Chats",
        screenTitle = "Chats",
        icon = MukeiIconKey.CHATS,
        emptyTitle = "Your conversations will appear here",
        emptyBody = "Pinned, recent, and project-linked chats will become available when durable conversation history is connected.",
    ),
    SETTINGS(
        drawerLabel = "Settings",
        screenTitle = "Settings",
        icon = MukeiIconKey.SETTINGS,
        emptyTitle = "Make Mukei yours",
        emptyBody = "Appearance, privacy, storage, providers, and advanced controls will be organized here as their product slices land.",
    ),
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
                text = "Opening your workspace…",
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
                text = "Your local data has not been opened for normal use. Close and reopen the app while this internal build has no safe in-process retry control yet.",
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
    val chatState = BackendRuntimeHost.chatState

    val navigateTo: (TopLevelDestination) -> Unit = { destination ->
        BackendRuntimeHost.leaveTemporaryChat {
            selectedName = destination.name
            closeDrawerRequest += 1
        }
    }

    LaunchedEffect(openDrawerRequest) {
        if (openDrawerRequest > 0) drawerState.open()
    }
    LaunchedEffect(closeDrawerRequest) {
        if (closeDrawerRequest > 0) drawerState.close()
    }

    BackHandler(
        enabled = drawerState.isOpen ||
            selected != TopLevelDestination.HOME ||
            chatState.temporary,
    ) {
        when {
            drawerState.isOpen -> closeDrawerRequest += 1
            selected != TopLevelDestination.HOME -> navigateTo(TopLevelDestination.HOME)
            chatState.temporary -> BackendRuntimeHost.leaveTemporaryChat { }
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
                            vertical = MukeiSpacing.Large,
                        ),
                    verticalArrangement = Arrangement.spacedBy(MukeiSpacing.Micro),
                ) {
                    DrawerDestinationItem(
                        destination = TopLevelDestination.HOME,
                        selected = selected,
                        onSelect = { navigateTo(TopLevelDestination.HOME) },
                    )
                    Spacer(Modifier.height(MukeiSpacing.Small))
                    listOf(
                        TopLevelDestination.STORAGE,
                        TopLevelDestination.PROJECTS,
                        TopLevelDestination.MODELS,
                    ).forEach { destination ->
                        DrawerDestinationItem(
                            destination = destination,
                            selected = selected,
                            onSelect = { navigateTo(destination) },
                        )
                    }
                    Spacer(Modifier.height(MukeiSpacing.Small))
                    DrawerDestinationItem(
                        destination = TopLevelDestination.CHATS,
                        selected = selected,
                        onSelect = { navigateTo(TopLevelDestination.CHATS) },
                    )
                    Spacer(Modifier.weight(1f))
                    DrawerDestinationItem(
                        destination = TopLevelDestination.SETTINGS,
                        selected = selected,
                        onSelect = { navigateTo(TopLevelDestination.SETTINGS) },
                    )
                }
            }
        },
    ) {
        Scaffold(
            containerColor = MaterialTheme.colorScheme.background,
            topBar = {
                TopAppBar(
                    colors = TopAppBarDefaults.topAppBarColors(
                        containerColor = MaterialTheme.colorScheme.background,
                        scrolledContainerColor = MaterialTheme.colorScheme.background,
                    ),
                    title = {
                        if (selected.screenTitle.isNotEmpty()) {
                            Text(
                                text = selected.screenTitle,
                                style = MaterialTheme.typography.titleLarge,
                            )
                        } else if (chatState.temporary) {
                            Text(
                                text = "Temporary Chat",
                                style = MaterialTheme.typography.titleMedium,
                            )
                        }
                    },
                    navigationIcon = {
                        IconButton(
                            onClick = { openDrawerRequest += 1 },
                            modifier = Modifier.semantics {
                                contentDescription = "Open navigation"
                            },
                        ) {
                            MukeiIcon(
                                icon = MukeiIconKey.MENU,
                                contentDescription = null,
                            )
                        }
                    },
                    actions = {
                        IconButton(
                            onClick = {
                                selectedName = TopLevelDestination.HOME.name
                                BackendRuntimeHost.startNewNormalChat()
                                newChatGeneration += 1
                            },
                            enabled = !chatState.transitionInProgress,
                            modifier = Modifier.semantics {
                                contentDescription = "New chat"
                            },
                        ) {
                            MukeiNewChatIcon(contentDescription = null)
                        }
                        if (selected == TopLevelDestination.HOME) {
                            IconButton(
                                onClick = BackendRuntimeHost::beginTemporaryChat,
                                enabled = state.readiness.inferenceReady &&
                                    !chatState.temporary &&
                                    !chatState.generationInProgress &&
                                    !chatState.transitionInProgress,
                                modifier = Modifier.semantics {
                                    contentDescription = if (chatState.temporary) {
                                        "Temporary chat active"
                                    } else {
                                        "Start temporary chat"
                                    }
                                },
                            ) {
                                MukeiIcon(
                                    icon = MukeiIconKey.TEMPORARY_CHAT,
                                    contentDescription = null,
                                    tint = if (chatState.temporary) {
                                        MaterialTheme.colorScheme.primary
                                    } else {
                                        MaterialTheme.colorScheme.onSurfaceVariant
                                    },
                                )
                            }
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
                        chatState = chatState,
                        resetGeneration = newChatGeneration,
                        openModels = { navigateTo(TopLevelDestination.MODELS) },
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
    val isSelected = selected == destination
    NavigationDrawerItem(
        icon = {
            MukeiIcon(
                icon = destination.icon,
                contentDescription = null,
                tint = if (isSelected) {
                    MaterialTheme.colorScheme.primary
                } else {
                    MaterialTheme.colorScheme.onSurfaceVariant
                },
            )
        },
        label = {
            Text(
                text = destination.drawerLabel,
                fontWeight = if (isSelected) FontWeight.Medium else FontWeight.Normal,
            )
        },
        selected = isSelected,
        onClick = onSelect,
        shape = MaterialTheme.shapes.medium,
    )
}

@Composable
private fun HomeSurface(
    readiness: AppReadiness,
    chatState: ChatUiState,
    resetGeneration: Int,
    openModels: () -> Unit,
) {
    var draft by rememberSaveable { mutableStateOf("") }
    var selectedCapabilityId by rememberSaveable { mutableStateOf<String?>(null) }
    val selectedCapability = HomeCapabilities.firstOrNull { it.id == selectedCapabilityId }
    val greeting = remember { homeGreeting(LocalTime.now().hour) }
    val transcriptScroll = rememberScrollState()

    LaunchedEffect(resetGeneration) {
        if (resetGeneration > 0) {
            draft = ""
            selectedCapabilityId = null
        }
    }
    LaunchedEffect(chatState.session?.conversationId, chatState.temporary) {
        if (chatState.temporary) {
            draft = ""
            selectedCapabilityId = null
        }
    }
    LaunchedEffect(chatState.messages.size, chatState.generationInProgress) {
        if (chatState.messages.isNotEmpty()) {
            transcriptScroll.animateScrollTo(transcriptScroll.maxValue)
        }
    }

    Column(
        modifier = Modifier
            .fillMaxSize()
            .padding(
                horizontal = MukeiLayout.LargePhoneTextPadding,
                vertical = MukeiSpacing.Large,
            ),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Column(
            modifier = Modifier
                .fillMaxWidth()
                .widthIn(max = MukeiLayout.ReadableContentMaxWidth),
        ) {
            if (chatState.temporary) {
                TemporaryChatNotice()
                Spacer(Modifier.height(MukeiSpacing.Medium))
            }
        }

        if (chatState.messages.isEmpty()) {
            Column(
                modifier = Modifier
                    .fillMaxWidth()
                    .widthIn(max = MukeiLayout.ReadableContentMaxWidth),
            ) {
                Spacer(Modifier.height(MukeiSpacing.LargeSection))
                Text(
                    text = if (chatState.temporary) "Private for this session." else greeting,
                    style = MaterialTheme.typography.titleMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                Spacer(Modifier.height(MukeiSpacing.ExtraSmall))
                Text(
                    text = "What’s on your mind?",
                    style = MaterialTheme.typography.headlineLarge,
                    color = MaterialTheme.colorScheme.onBackground,
                )
            }
            Spacer(Modifier.weight(1f))
        } else {
            Column(
                modifier = Modifier
                    .weight(1f)
                    .fillMaxWidth()
                    .widthIn(max = MukeiLayout.ReadableContentMaxWidth)
                    .verticalScroll(transcriptScroll),
                verticalArrangement = Arrangement.spacedBy(MukeiSpacing.Small),
            ) {
                chatState.messages.forEach { message ->
                    ChatMessageBubble(message)
                }
                if (chatState.generationInProgress &&
                    chatState.messages.lastOrNull()?.streaming != true
                ) {
                    Text(
                        text = "Thinking…",
                        style = MaterialTheme.typography.bodyMedium,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
                Spacer(Modifier.height(MukeiSpacing.Small))
            }
        }

        Column(
            modifier = Modifier
                .fillMaxWidth()
                .widthIn(max = MukeiLayout.ReadableContentMaxWidth),
        ) {
            Row(
                modifier = Modifier
                    .fillMaxWidth()
                    .horizontalScroll(rememberScrollState()),
                horizontalArrangement = Arrangement.spacedBy(MukeiSpacing.ExtraSmall),
            ) {
                HomeCapabilities.forEach { capability ->
                    val available = !chatState.temporary ||
                        (capability.id != "research" && capability.id != "files")
                    MukeiCapabilityChip(
                        label = capability.label,
                        selected = selectedCapabilityId == capability.id,
                        enabled = available,
                        onClick = {
                            selectedCapabilityId = if (selectedCapabilityId == capability.id) {
                                null
                            } else {
                                capability.id
                            }
                        },
                    )
                }
            }

            Spacer(Modifier.height(MukeiSpacing.Medium))

            MukeiComposer(
                draft = draft,
                onDraftChange = { draft = it },
                placeholder = selectedCapability?.placeholder ?: "Tell Mukei what you want to do…",
                temporary = chatState.temporary,
                sendEnabled = readiness.inferenceReady &&
                    draft.isNotBlank() &&
                    !chatState.generationInProgress &&
                    !chatState.transitionInProgress,
                onSend = {
                    val submitted = draft
                    if (submitted.isNotBlank()) {
                        BackendRuntimeHost.submitChatMessage(submitted)
                        draft = ""
                    }
                },
            )

            chatState.errorCode?.let { code ->
                Spacer(Modifier.height(MukeiSpacing.Small))
                ChatErrorNotice(code)
            }

            if (readiness.inference.status == ReadinessStatus.ACTION_REQUIRED) {
                Spacer(Modifier.height(MukeiSpacing.Small))
                ModelSetupNotice(openModels = openModels)
            }

            Spacer(Modifier.height(MukeiSpacing.ExtraSmall))
        }
    }
}

@Composable
private fun TemporaryChatNotice() {
    Surface(
        modifier = Modifier.fillMaxWidth(),
        shape = MaterialTheme.shapes.medium,
        color = MaterialTheme.colorScheme.surfaceVariant,
        border = BorderStroke(MukeiStroke.Thin, MaterialTheme.colorScheme.outline),
    ) {
        Row(
            modifier = Modifier.padding(MukeiSpacing.Medium),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(MukeiSpacing.Small),
        ) {
            MukeiIcon(
                icon = MukeiIconKey.TEMPORARY_CHAT,
                contentDescription = null,
                tint = MaterialTheme.colorScheme.primary,
            )
            Column {
                Text(
                    text = "Temporary Chat",
                    style = MaterialTheme.typography.titleSmall,
                    fontWeight = FontWeight.SemiBold,
                )
                Text(
                    text = "Not saved. RAG, files, and web tools are off. Leaving this chat discards it.",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
    }
}

@Composable
private fun ChatMessageBubble(message: ChatUiMessage) {
    val user = message.role == ChatMessageRole.USER
    Row(
        modifier = Modifier.fillMaxWidth(),
        horizontalArrangement = if (user) Arrangement.End else Arrangement.Start,
    ) {
        Surface(
            modifier = Modifier.fillMaxWidth(0.88f),
            shape = MaterialTheme.shapes.large,
            color = if (user) {
                MaterialTheme.colorScheme.primaryContainer
            } else {
                MaterialTheme.colorScheme.surfaceVariant
            },
        ) {
            Text(
                text = message.text,
                modifier = Modifier.padding(MukeiSpacing.Medium),
                style = MaterialTheme.typography.bodyLarge,
                color = if (user) {
                    MaterialTheme.colorScheme.onPrimaryContainer
                } else {
                    MaterialTheme.colorScheme.onSurfaceVariant
                },
            )
        }
    }
}

@Composable
private fun ChatErrorNotice(code: String) {
    Surface(
        modifier = Modifier.fillMaxWidth(),
        shape = MaterialTheme.shapes.medium,
        color = MaterialTheme.colorScheme.errorContainer,
    ) {
        Row(
            modifier = Modifier.padding(MukeiSpacing.Medium),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Column(modifier = Modifier.weight(1f)) {
                Text(
                    text = "Mukei couldn’t complete that action.",
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onErrorContainer,
                )
                Text(
                    text = "Diagnostic: $code",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onErrorContainer,
                )
            }
            TextButton(onClick = BackendRuntimeHost::clearChatError) {
                Text("Dismiss")
            }
        }
    }
}

@Composable
private fun MukeiComposer(
    draft: String,
    onDraftChange: (String) -> Unit,
    placeholder: String,
    temporary: Boolean,
    sendEnabled: Boolean,
    onSend: () -> Unit,
) {
    Surface(
        modifier = Modifier.fillMaxWidth(),
        shape = RoundedCornerShape(MukeiRadius.Composer),
        color = MaterialTheme.colorScheme.surface,
        border = BorderStroke(
            width = MukeiStroke.Thin,
            color = MaterialTheme.colorScheme.outline,
        ),
    ) {
        Column(
            modifier = Modifier.padding(MukeiSpacing.Comfortable),
        ) {
            BasicTextField(
                value = draft,
                onValueChange = onDraftChange,
                modifier = Modifier.fillMaxWidth(),
                textStyle = MaterialTheme.typography.bodyLarge.copy(
                    color = MaterialTheme.colorScheme.onSurface,
                ),
                cursorBrush = SolidColor(MaterialTheme.colorScheme.primary),
                minLines = 2,
                maxLines = 6,
                decorationBox = { innerTextField ->
                    Box(modifier = Modifier.fillMaxWidth()) {
                        if (draft.isEmpty()) {
                            Text(
                                text = placeholder,
                                style = MaterialTheme.typography.bodyLarge,
                                color = MaterialTheme.colorScheme.onSurfaceVariant,
                            )
                        }
                        innerTextField()
                    }
                },
            )
            Spacer(Modifier.height(MukeiSpacing.ExtraSmall))
            Row(
                modifier = Modifier.fillMaxWidth(),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                IconButton(
                    onClick = {},
                    enabled = false,
                    modifier = Modifier.semantics {
                        contentDescription = if (temporary) {
                            "Attachments unavailable in Temporary Chat"
                        } else {
                            "Attachments unavailable in this build"
                        }
                    },
                ) {
                    MukeiIcon(
                        icon = MukeiIconKey.ATTACH,
                        contentDescription = null,
                    )
                }
                Spacer(Modifier.weight(1f))
                Surface(
                    shape = MaterialTheme.shapes.extraLarge,
                    color = if (sendEnabled) {
                        MaterialTheme.colorScheme.primaryContainer
                    } else {
                        MaterialTheme.colorScheme.surfaceVariant
                    },
                ) {
                    IconButton(
                        onClick = onSend,
                        enabled = sendEnabled,
                        modifier = Modifier.semantics {
                            contentDescription = if (sendEnabled) {
                                "Send message"
                            } else {
                                "Send unavailable"
                            }
                        },
                    ) {
                        MukeiIcon(
                            icon = MukeiIconKey.SEND,
                            contentDescription = null,
                        )
                    }
                }
            }
        }
    }
}

@Composable
private fun MukeiCapabilityChip(
    label: String,
    selected: Boolean,
    enabled: Boolean,
    onClick: () -> Unit,
) {
    val container = if (selected) {
        MaterialTheme.colorScheme.primaryContainer
    } else {
        MaterialTheme.colorScheme.surface
    }
    val border = if (selected) {
        MaterialTheme.colorScheme.primary
    } else {
        MaterialTheme.colorScheme.outline
    }
    Surface(
        modifier = Modifier
            .heightIn(min = MukeiLayout.MinimumTouchTarget)
            .clickable(enabled = enabled, onClick = onClick),
        shape = MaterialTheme.shapes.small,
        color = container,
        border = BorderStroke(
            width = MukeiStroke.Thin,
            color = border,
        ),
    ) {
        Box(
            modifier = Modifier.padding(
                horizontal = MukeiSpacing.Medium,
                vertical = MukeiSpacing.Small,
            ),
            contentAlignment = Alignment.Center,
        ) {
            Text(
                text = label,
                style = MaterialTheme.typography.labelLarge,
                color = if (enabled) {
                    MaterialTheme.colorScheme.onSurface
                } else {
                    MaterialTheme.colorScheme.onSurfaceVariant
                },
                fontWeight = if (selected) FontWeight.SemiBold else FontWeight.Normal,
            )
        }
    }
}

@Composable
private fun ModelSetupNotice(
    openModels: () -> Unit,
) {
    Surface(
        modifier = Modifier.fillMaxWidth(),
        shape = MaterialTheme.shapes.medium,
        color = MaterialTheme.colorScheme.surfaceVariant,
    ) {
        Row(
            modifier = Modifier.padding(
                horizontal = MukeiSpacing.Medium,
                vertical = MukeiSpacing.ExtraSmall,
            ),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(MukeiSpacing.Small),
        ) {
            MukeiIcon(
                icon = MukeiIconKey.MODELS,
                contentDescription = null,
            )
            Text(
                text = "Model required for replies",
                modifier = Modifier.weight(1f),
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            TextButton(onClick = openModels) {
                Text("Models")
            }
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
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Column(
            modifier = Modifier
                .fillMaxWidth()
                .widthIn(max = MukeiLayout.ReadableContentMaxWidth),
        ) {
            Text(
                text = "Models",
                style = MaterialTheme.typography.headlineMedium,
            )
            Spacer(Modifier.height(MukeiSpacing.Large))
            Surface(
                modifier = Modifier.fillMaxWidth(),
                shape = MaterialTheme.shapes.large,
                color = MaterialTheme.colorScheme.surface,
                border = BorderStroke(
                    width = MukeiStroke.Thin,
                    color = MaterialTheme.colorScheme.outline,
                ),
            ) {
                Column(
                    modifier = Modifier.padding(MukeiSpacing.Large),
                    horizontalAlignment = Alignment.Start,
                ) {
                    Surface(
                        shape = MaterialTheme.shapes.medium,
                        color = MaterialTheme.colorScheme.surfaceVariant,
                    ) {
                        Box(modifier = Modifier.padding(MukeiSpacing.Small)) {
                            MukeiIcon(
                                icon = MukeiIconKey.MODELS,
                                contentDescription = null,
                            )
                        }
                    }
                    Spacer(Modifier.height(MukeiSpacing.Medium))
                    Text(
                        text = when (readiness.inference.status) {
                            ReadinessStatus.READY -> "Inference is ready"
                            ReadinessStatus.ACTION_REQUIRED -> "No active model yet"
                            ReadinessStatus.DEGRADED -> "Model capability needs attention"
                            ReadinessStatus.UNAVAILABLE -> "Model capability is unavailable"
                            ReadinessStatus.UNKNOWN -> "Checking model capability"
                        },
                        style = MaterialTheme.typography.titleLarge,
                    )
                    Spacer(Modifier.height(MukeiSpacing.ExtraSmall))
                    Text(
                        text = when (readiness.inference.status) {
                            ReadinessStatus.READY -> "The runtime reports that conversation inference is available."
                            ReadinessStatus.ACTION_REQUIRED -> "Local inference artifacts are required before Mukei can respond. Installation controls are not exposed in this internal build yet."
                            ReadinessStatus.DEGRADED -> "The inference runtime is partially available and needs recovery before normal use."
                            ReadinessStatus.UNAVAILABLE -> "This runtime cannot currently provide conversation inference."
                            ReadinessStatus.UNKNOWN -> "Mukei is still resolving the current inference capability state."
                        },
                        style = MaterialTheme.typography.bodyLarge,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            }
        }
    }
}

@Composable
private fun ReservedDestinationSurface(destination: TopLevelDestination) {
    Column(
        modifier = Modifier
            .fillMaxSize()
            .verticalScroll(rememberScrollState())
            .padding(MukeiLayout.LargePhoneTextPadding),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Spacer(Modifier.height(MukeiSpacing.Major))
        Column(
            modifier = Modifier
                .fillMaxWidth()
                .widthIn(max = MukeiLayout.ReadableContentMaxWidth),
            horizontalAlignment = Alignment.CenterHorizontally,
        ) {
            Surface(
                shape = MaterialTheme.shapes.large,
                color = MaterialTheme.colorScheme.surfaceVariant,
            ) {
                Box(modifier = Modifier.padding(MukeiSpacing.Medium)) {
                    MukeiIcon(
                        icon = destination.icon,
                        contentDescription = null,
                    )
                }
            }
            Spacer(Modifier.height(MukeiSpacing.Large))
            Text(
                text = destination.emptyTitle,
                style = MaterialTheme.typography.headlineSmall,
                textAlign = TextAlign.Center,
            )
            Spacer(Modifier.height(MukeiSpacing.ExtraSmall))
            Text(
                text = destination.emptyBody,
                style = MaterialTheme.typography.bodyLarge,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                textAlign = TextAlign.Center,
            )
        }
    }
}

internal fun homeGreeting(hour: Int): String = when (hour) {
    in 5..11 -> "Good morning."
    in 12..16 -> "Good afternoon."
    in 17..22 -> "Good evening."
    else -> "Ready when you are."
}
