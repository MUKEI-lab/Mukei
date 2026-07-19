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
import ai.mukei.android.designsystem.MukeiNewChatIcon
import ai.mukei.android.designsystem.MukeiRadius
import ai.mukei.android.designsystem.MukeiSpacing
import ai.mukei.android.designsystem.MukeiStroke
import java.time.LocalTime

private data class ReadyHomeCapability(
    val id: String,
    val label: String,
    val placeholder: String,
)

private val ReadyHomeCapabilities = listOf(
    ReadyHomeCapability("research", "Deep Research", "What should Mukei research?"),
    ReadyHomeCapability("build", "Build App", "Describe the app you want made…"),
    ReadyHomeCapability("files", "Read Files", "What should Mukei do with your files?"),
    ReadyHomeCapability("write", "Write", "What should we write?"),
    ReadyHomeCapability("code", "Code", "Describe what you want to build or fix…"),
)

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun MukeiReadyProductShell(state: BackendRuntimeHost.State.Ready) {
    var selectedName by rememberSaveable { mutableStateOf(TopLevelDestination.HOME.name) }
    val selected = TopLevelDestination.valueOf(selectedName)
    val drawerState = rememberDrawerState(initialValue = DrawerValue.Closed)
    var openDrawerRequest by remember { mutableIntStateOf(0) }
    var closeDrawerRequest by remember { mutableIntStateOf(0) }
    var newChatGeneration by rememberSaveable { mutableIntStateOf(0) }
    val conversation = BackendRuntimeHost.conversationState

    fun selectDestination(destination: TopLevelDestination) {
        if (conversation.temporary) {
            BackendRuntimeHost.exitTemporaryChat { ended ->
                if (ended) {
                    selectedName = destination.name
                    newChatGeneration += 1
                    closeDrawerRequest += 1
                }
            }
        } else {
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
        enabled = drawerState.isOpen || conversation.temporary || selected != TopLevelDestination.HOME,
    ) {
        when {
            drawerState.isOpen -> closeDrawerRequest += 1
            conversation.temporary -> {
                BackendRuntimeHost.exitTemporaryChat { ended ->
                    if (ended) newChatGeneration += 1
                }
            }
            else -> selectedName = TopLevelDestination.HOME.name
        }
    }

    ModalNavigationDrawer(
        drawerState = drawerState,
        gesturesEnabled = !conversation.transitionInProgress,
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
                    ReadyDrawerDestinationItem(
                        destination = TopLevelDestination.HOME,
                        selected = selected,
                        enabled = !conversation.transitionInProgress,
                        onSelect = { selectDestination(TopLevelDestination.HOME) },
                    )
                    Spacer(Modifier.height(MukeiSpacing.Small))
                    listOf(
                        TopLevelDestination.STORAGE,
                        TopLevelDestination.PROJECTS,
                        TopLevelDestination.MODELS,
                    ).forEach { destination ->
                        ReadyDrawerDestinationItem(
                            destination = destination,
                            selected = selected,
                            enabled = !conversation.transitionInProgress,
                            onSelect = { selectDestination(destination) },
                        )
                    }
                    Spacer(Modifier.height(MukeiSpacing.Small))
                    ReadyDrawerDestinationItem(
                        destination = TopLevelDestination.CHATS,
                        selected = selected,
                        enabled = !conversation.transitionInProgress,
                        onSelect = { selectDestination(TopLevelDestination.CHATS) },
                    )
                    Spacer(Modifier.weight(1f))
                    ReadyDrawerDestinationItem(
                        destination = TopLevelDestination.SETTINGS,
                        selected = selected,
                        enabled = !conversation.transitionInProgress,
                        onSelect = { selectDestination(TopLevelDestination.SETTINGS) },
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
                        when {
                            conversation.temporary && selected == TopLevelDestination.HOME -> {
                                Text(
                                    text = "Temporary Chat",
                                    style = MaterialTheme.typography.titleLarge,
                                )
                            }
                            selected.screenTitle.isNotEmpty() -> {
                                Text(
                                    text = selected.screenTitle,
                                    style = MaterialTheme.typography.titleLarge,
                                )
                            }
                        }
                    },
                    navigationIcon = {
                        IconButton(
                            onClick = { openDrawerRequest += 1 },
                            enabled = !conversation.transitionInProgress,
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
                                BackendRuntimeHost.startNewChat { ready ->
                                    if (ready) newChatGeneration += 1
                                }
                            },
                            enabled = !conversation.busy,
                            modifier = Modifier.semantics {
                                contentDescription = "New chat"
                            },
                        ) {
                            MukeiNewChatIcon(contentDescription = null)
                        }
                        IconButton(
                            onClick = {
                                selectedName = TopLevelDestination.HOME.name
                                if (conversation.temporary) {
                                    BackendRuntimeHost.exitTemporaryChat { ended ->
                                        if (ended) newChatGeneration += 1
                                    }
                                } else {
                                    BackendRuntimeHost.startTemporaryChat { started ->
                                        if (started) newChatGeneration += 1
                                    }
                                }
                            },
                            enabled = if (conversation.temporary) {
                                !conversation.transitionInProgress
                            } else {
                                BackendRuntimeHost.temporaryChatAvailable && !conversation.busy
                            },
                            modifier = Modifier.semantics {
                                contentDescription = if (conversation.temporary) {
                                    "Exit temporary chat"
                                } else {
                                    "Start temporary chat"
                                }
                            },
                        ) {
                            MukeiIcon(
                                icon = MukeiIconKey.TEMPORARY_CHAT,
                                contentDescription = null,
                                tint = if (conversation.temporary) {
                                    MaterialTheme.colorScheme.primary
                                } else {
                                    MaterialTheme.colorScheme.onSurfaceVariant
                                },
                            )
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
                    TopLevelDestination.HOME -> ReadyConversationHomeSurface(
                        readiness = state.readiness,
                        conversation = conversation,
                        resetGeneration = newChatGeneration,
                        openModels = { selectDestination(TopLevelDestination.MODELS) },
                    )
                    TopLevelDestination.MODELS -> ReadyModelsSurface(state.readiness)
                    else -> ReadyReservedDestinationSurface(selected)
                }
            }
        }
    }
}

@Composable
private fun ReadyDrawerDestinationItem(
    destination: TopLevelDestination,
    selected: TopLevelDestination,
    enabled: Boolean,
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
        enabled = enabled,
        shape = MaterialTheme.shapes.medium,
    )
}

@Composable
private fun ReadyConversationHomeSurface(
    readiness: AppReadiness,
    conversation: BackendRuntimeHost.ConversationState,
    resetGeneration: Int,
    openModels: () -> Unit,
) {
    var draft by rememberSaveable { mutableStateOf("") }
    var selectedCapabilityId by rememberSaveable { mutableStateOf<String?>(null) }
    val selectedCapability = ReadyHomeCapabilities.firstOrNull { it.id == selectedCapabilityId }
    val greeting = remember { homeGreeting(LocalTime.now().hour) }
    val transcriptScroll = rememberScrollState()
    val hasTranscript = conversation.messages.isNotEmpty() || conversation.streamingAssistant.isNotEmpty()

    LaunchedEffect(resetGeneration, conversation.conversationId) {
        draft = ""
        selectedCapabilityId = null
    }
    LaunchedEffect(conversation.messages.size, conversation.streamingAssistant.length) {
        if (hasTranscript) transcriptScroll.animateScrollTo(transcriptScroll.maxValue)
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
            if (conversation.temporary) {
                ReadyTemporaryChatNotice(transitionInProgress = conversation.transitionInProgress)
                Spacer(Modifier.height(MukeiSpacing.Medium))
            }

            if (!hasTranscript) {
                Spacer(Modifier.height(MukeiSpacing.LargeSection))
                Text(
                    text = greeting,
                    style = MaterialTheme.typography.titleMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                Spacer(Modifier.height(MukeiSpacing.ExtraSmall))
                Text(
                    text = if (conversation.temporary) {
                        "What should stay temporary?"
                    } else {
                        "What’s on your mind?"
                    },
                    style = MaterialTheme.typography.headlineLarge,
                    color = MaterialTheme.colorScheme.onBackground,
                )
            }
        }

        if (hasTranscript) {
            Column(
                modifier = Modifier
                    .weight(1f)
                    .fillMaxWidth()
                    .widthIn(max = MukeiLayout.ReadableContentMaxWidth)
                    .verticalScroll(transcriptScroll)
                    .padding(vertical = MukeiSpacing.Medium),
                verticalArrangement = Arrangement.spacedBy(MukeiSpacing.Small),
            ) {
                conversation.messages.forEach { message ->
                    ReadyConversationMessageBubble(message)
                }
                if (conversation.streamingAssistant.isNotEmpty()) {
                    ReadyAssistantBubble(
                        text = conversation.streamingAssistant,
                        streaming = true,
                    )
                }
            }
        } else {
            Spacer(Modifier.weight(1f))
        }

        Column(
            modifier = Modifier
                .fillMaxWidth()
                .widthIn(max = MukeiLayout.ReadableContentMaxWidth),
        ) {
            if (!conversation.temporary && !hasTranscript) {
                Row(
                    modifier = Modifier
                        .fillMaxWidth()
                        .horizontalScroll(rememberScrollState()),
                    horizontalArrangement = Arrangement.spacedBy(MukeiSpacing.ExtraSmall),
                ) {
                    ReadyHomeCapabilities.forEach { capability ->
                        ReadyCapabilityChip(
                            label = capability.label,
                            selected = selectedCapabilityId == capability.id,
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
            }

            ReadyComposer(
                draft = draft,
                onDraftChange = { draft = it },
                placeholder = if (conversation.temporary) {
                    "Message Mukei privately for this session…"
                } else {
                    selectedCapability?.placeholder ?: "Tell Mukei what you want to do…"
                },
                sendEnabled = readiness.inferenceReady && !conversation.busy && draft.isNotBlank(),
                temporary = conversation.temporary,
                onSend = {
                    if (BackendRuntimeHost.sendMessage(draft)) {
                        draft = ""
                        selectedCapabilityId = null
                    }
                },
            )

            conversation.lastErrorCode?.let { code ->
                Spacer(Modifier.height(MukeiSpacing.Small))
                Text(
                    text = "Request unavailable · $code",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.error,
                )
            }

            if (!readiness.inferenceReady) {
                Spacer(Modifier.height(MukeiSpacing.Small))
                ReadyModelSetupNotice(openModels = openModels)
            }

            Spacer(Modifier.height(MukeiSpacing.ExtraSmall))
        }
    }
}

@Composable
private fun ReadyTemporaryChatNotice(transitionInProgress: Boolean) {
    Surface(
        modifier = Modifier.fillMaxWidth(),
        shape = MaterialTheme.shapes.medium,
        color = MaterialTheme.colorScheme.primaryContainer,
    ) {
        Column(
            modifier = Modifier.padding(MukeiSpacing.Medium),
        ) {
            Text(
                text = "Temporary Chat",
                style = MaterialTheme.typography.titleMedium,
                color = MaterialTheme.colorScheme.onPrimaryContainer,
            )
            Spacer(Modifier.height(MukeiSpacing.Micro))
            Text(
                text = "Not saved. RAG, file access, web search, and attachments are off. Leaving this chat purges its in-memory session.",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onPrimaryContainer,
            )
            if (transitionInProgress) {
                Spacer(Modifier.height(MukeiSpacing.Small))
                LinearProgressIndicator(modifier = Modifier.fillMaxWidth())
            }
        }
    }
}

@Composable
private fun ReadyConversationMessageBubble(message: BackendRuntimeHost.ChatMessage) {
    when (message.role) {
        BackendRuntimeHost.ChatRole.USER -> {
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.End,
            ) {
                Surface(
                    modifier = Modifier.widthIn(max = MukeiLayout.ReadableContentMaxWidth),
                    shape = MaterialTheme.shapes.large,
                    color = MaterialTheme.colorScheme.primaryContainer,
                ) {
                    Text(
                        text = message.text,
                        modifier = Modifier.padding(MukeiSpacing.Medium),
                        style = MaterialTheme.typography.bodyLarge,
                        color = MaterialTheme.colorScheme.onPrimaryContainer,
                    )
                }
            }
        }
        BackendRuntimeHost.ChatRole.ASSISTANT -> ReadyAssistantBubble(message.text, streaming = false)
    }
}

@Composable
private fun ReadyAssistantBubble(
    text: String,
    streaming: Boolean,
) {
    Row(
        modifier = Modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.Start,
    ) {
        Surface(
            modifier = Modifier.widthIn(max = MukeiLayout.ReadableContentMaxWidth),
            shape = MaterialTheme.shapes.large,
            color = MaterialTheme.colorScheme.surface,
            border = BorderStroke(MukeiStroke.Thin, MaterialTheme.colorScheme.outline),
        ) {
            Column(modifier = Modifier.padding(MukeiSpacing.Medium)) {
                Text(
                    text = text,
                    style = MaterialTheme.typography.bodyLarge,
                    color = MaterialTheme.colorScheme.onSurface,
                )
                if (streaming) {
                    Spacer(Modifier.height(MukeiSpacing.ExtraSmall))
                    LinearProgressIndicator(modifier = Modifier.fillMaxWidth())
                }
            }
        }
    }
}

@Composable
private fun ReadyComposer(
    draft: String,
    onDraftChange: (String) -> Unit,
    placeholder: String,
    sendEnabled: Boolean,
    temporary: Boolean,
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
private fun ReadyCapabilityChip(
    label: String,
    selected: Boolean,
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
            .clickable(onClick = onClick),
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
                fontWeight = if (selected) FontWeight.SemiBold else FontWeight.Normal,
            )
        }
    }
}

@Composable
private fun ReadyModelSetupNotice(openModels: () -> Unit) {
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
private fun ReadyModelsSurface(readiness: AppReadiness) {
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
private fun ReadyReservedDestinationSurface(destination: TopLevelDestination) {
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
