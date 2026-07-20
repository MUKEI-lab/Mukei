package ai.mukei.android

import android.content.Context
import android.os.Handler
import android.os.Looper
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import ai.mukei.android.core.nativebridge.AndroidPlatformRequestProcessor
import ai.mukei.android.core.nativebridge.RustNativeGateway
import ai.mukei.android.core.nativebridge.SecureRuntimeFactory
import ai.mukei.android.protocol.AcknowledgementStatus
import ai.mukei.android.protocol.CommandEnvelopeV2
import ai.mukei.android.protocol.CommandScope
import ai.mukei.android.protocol.EventEnvelopeV2
import ai.mukei.android.protocol.EventSequenceTracker
import ai.mukei.android.protocol.ProtocolJsonCodec
import ai.mukei.android.protocol.ProtocolVersion
import ai.mukei.android.protocol.RuntimeContractJsonCodec
import ai.mukei.android.protocol.RuntimeContractSnapshot
import java.io.Closeable
import java.io.File
import java.nio.charset.StandardCharsets
import java.time.Instant
import java.util.UUID
import java.util.concurrent.CopyOnWriteArraySet
import java.util.concurrent.Executors
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicInteger
import java.util.concurrent.atomic.AtomicReference
import org.json.JSONObject

/** Process-scoped owner for the native runtime, typed event stream and Android broker. */
object BackendRuntimeHost {
    sealed interface State {
        data object Starting : State

        data class Ready(
            val readiness: AppReadiness,
            val runtimeContract: RuntimeContractSnapshot,
        ) : State

        data class Failed(val code: String) : State
        data object Stopped : State
    }

    enum class ChatRole {
        USER,
        ASSISTANT,
    }

    data class ChatMessage(
        val id: String,
        val role: ChatRole,
        val text: String,
    )

    data class ConversationState(
        val conversationId: String,
        val branchId: String,
        val temporary: Boolean,
        val ragEnabled: Boolean,
        val messages: List<ChatMessage> = emptyList(),
        val streamingAssistant: String = "",
        val activeOperationId: String? = null,
        val transitionInProgress: Boolean = false,
        val lastErrorCode: String? = null,
    ) {
        val busy: Boolean
            get() = activeOperationId != null || transitionInProgress
    }

    data class RuntimeEventBatch(
        val runtimeSessionId: String,
        val events: List<EventEnvelopeV2>,
        val hasMore: Boolean,
        val sequenceGapCount: Int,
        val runtimeSessionChanged: Boolean,
    )

    data class EventStatus(
        val runtimeSessionId: String? = null,
        val deliveredEvents: Long = 0,
        val lastEventType: String? = null,
        val detectedSequenceGaps: Long = 0,
        val runtimeSessionChanges: Long = 0,
    )

    fun interface EventBatchListener {
        fun onEvents(batch: RuntimeEventBatch)
    }

    private val started = AtomicBoolean(false)
    private val bootstrapActive = AtomicBoolean(false)
    private val running = AtomicBoolean(false)
    private val activeWorkers = AtomicInteger(0)
    private val terminalFailure = AtomicReference<String?>(null)
    private val mainHandler = Handler(Looper.getMainLooper())

    private val bootstrapExecutor = Executors.newSingleThreadExecutor { runnable ->
        Thread(runnable, "mukei-backend-bootstrap").apply { isDaemon = true }
    }
    private val platformExecutor = Executors.newSingleThreadExecutor { runnable ->
        Thread(runnable, "mukei-platform-broker").apply { isDaemon = true }
    }
    private val eventExecutor = Executors.newSingleThreadExecutor { runnable ->
        Thread(runnable, "mukei-event-dispatcher").apply { isDaemon = true }
    }
    private val commandExecutor = Executors.newSingleThreadExecutor { runnable ->
        Thread(runnable, "mukei-command-dispatcher").apply { isDaemon = true }
    }

    private val gateway = AtomicReference<RustNativeGateway?>(null)
    private val eventListeners = CopyOnWriteArraySet<EventBatchListener>()
    private val eventSequenceTracker = EventSequenceTracker()

    var state: State by mutableStateOf<State>(State.Starting)
        private set

    var eventStatus: EventStatus by mutableStateOf(EventStatus())
        private set

    var conversationState: ConversationState by mutableStateOf(newDurableConversation())
        private set

    val temporaryChatAvailable: Boolean
        get() {
            val ready = state as? State.Ready ?: return false
            val capabilities = ready.runtimeContract.protocol.capabilities
            return CAP_TEMPORARY_CHAT_SESSIONS in capabilities &&
                CAP_TEMPORARY_CHAT_RAG_DISABLED in capabilities
        }

    fun addEventListener(listener: EventBatchListener): Closeable {
        eventListeners.add(listener)
        return object : Closeable {
            override fun close() {
                eventListeners.remove(listener)
            }
        }
    }

    fun start(context: Context) {
        if (!started.compareAndSet(false, true)) return

        bootstrapActive.set(true)
        running.set(true)
        terminalFailure.set(null)
        eventSequenceTracker.reset()
        resetEventStatus()
        publish(State.Starting)

        val appContext = context.applicationContext
        try {
            bootstrapExecutor.execute {
                var ownedGateway: RustNativeGateway? = null
                var workersLaunched = false
                try {
                    val dataRoot = File(appContext.filesDir, "mukei").canonicalFile
                    if (!dataRoot.exists() && !dataRoot.mkdirs()) {
                        throw IllegalStateException("app_data_directory_unavailable")
                    }
                    val configPath = File(dataRoot, "mukei.toml").canonicalFile
                    val configJson = JSONObject()
                        .put("app_data_dir", dataRoot.absolutePath)
                        .put("worker_threads", 2)
                        .put("max_blocking_threads", 6)
                        .put("event_capacity", 512)
                        .toString()
                        .toByteArray(StandardCharsets.UTF_8)

                    val activeGateway = SecureRuntimeFactory.open(appContext, configJson)
                    ownedGateway = activeGateway

                    val acknowledgement = ProtocolJsonCodec.decodeAcknowledgement(
                        activeGateway.submitCommand(initializeEnvelope(configPath)),
                    )
                    if (acknowledgement.status != AcknowledgementStatus.ACCEPTED) {
                        throw IllegalStateException(
                            acknowledgement.rejectionReason ?: "initialize_rejected",
                        )
                    }
                    if (!running.get()) return@execute

                    gateway.set(activeGateway)
                    val runtimeContract = RuntimeContractJsonCodec.decode(
                        activeGateway.protocolCapabilities(),
                    )
                    val readiness = AppReadiness.fromSecurityStatus(activeGateway.securityStatus())
                    if (!readiness.shellUsable) {
                        throw IllegalStateException("secure_storage_not_ready")
                    }
                    if (!running.get()) return@execute

                    publish(
                        State.Ready(
                            readiness = readiness,
                            runtimeContract = runtimeContract,
                        ),
                    )

                    launchWorkers(appContext, activeGateway)
                    workersLaunched = true
                } catch (failure: Throwable) {
                    running.set(false)
                    terminalFailure.compareAndSet(null, stableFailureCode(failure))
                } finally {
                    if (!workersLaunched) {
                        ownedGateway?.let { activeGateway ->
                            gateway.compareAndSet(activeGateway, null)
                            activeGateway.runCatching { close() }
                        }
                        eventSequenceTracker.reset()
                        started.set(false)
                        val failure = terminalFailure.get()
                        bootstrapActive.set(false)
                        publish(if (failure == null) State.Stopped else State.Failed(failure))
                    } else {
                        bootstrapActive.set(false)
                    }
                }
            }
        } catch (failure: Throwable) {
            running.set(false)
            terminalFailure.compareAndSet(null, stableFailureCode(failure))
            started.set(false)
            bootstrapActive.set(false)
            publish(State.Failed(terminalFailure.get() ?: "backend_runtime_failed"))
        }
    }

    fun startNewChat(onReady: (Boolean) -> Unit = {}): Boolean {
        val current = conversationState
        if (current.busy) return false

        if (!current.temporary) {
            conversationState = newDurableConversation()
            onReady(true)
            return true
        }
        return exitTemporaryChat(onReady)
    }

    fun startTemporaryChat(onReady: (Boolean) -> Unit = {}): Boolean {
        val ready = state as? State.Ready ?: return false
        val current = conversationState
        if (current.busy || current.temporary || !temporaryChatAvailable) return false
        val activeGateway = gateway.get() ?: return false

        conversationState = current.copy(
            transitionInProgress = true,
            lastErrorCode = null,
        )

        return try {
            commandExecutor.execute {
                val result = runCatching {
                    val session = activeGateway.beginTemporaryChat()
                    if (!BackendRuntimePolicies.temporaryStartMatches(
                            session = session,
                            expectedRuntimeSessionId = ready.runtimeContract.runtimeSessionId,
                        )
                    ) {
                        val ended = activeGateway.endTemporaryChat(
                            session.conversationId,
                            session.branchId,
                        )
                        if (!BackendRuntimePolicies.temporaryCleanupMatches(ended, session)) {
                            throw IllegalStateException("temporary_chat_cleanup_failed")
                        }
                        throw IllegalStateException("temporary_chat_contract_mismatch")
                    }
                    session
                }
                mainHandler.post {
                    val latest = conversationState
                    if (latest.conversationId != current.conversationId ||
                        !latest.transitionInProgress
                    ) {
                        return@post
                    }

                    result.fold(
                        onSuccess = { session ->
                            conversationState = ConversationState(
                                conversationId = session.conversationId,
                                branchId = session.branchId,
                                temporary = true,
                                ragEnabled = false,
                            )
                            onReady(true)
                        },
                        onFailure = { failure ->
                            conversationState = current.copy(
                                transitionInProgress = false,
                                lastErrorCode = stableFailureCode(failure),
                            )
                            onReady(false)
                        },
                    )
                }
            }
            true
        } catch (failure: Throwable) {
            conversationState = current.copy(
                transitionInProgress = false,
                lastErrorCode = stableFailureCode(failure),
            )
            onReady(false)
            false
        }
    }

    fun exitTemporaryChat(onEnded: (Boolean) -> Unit = {}): Boolean {
        val ready = state as? State.Ready ?: return false
        val current = conversationState
        if (!current.temporary || current.transitionInProgress) return false
        val activeGateway = gateway.get() ?: return false

        conversationState = current.copy(
            transitionInProgress = true,
            lastErrorCode = null,
        )

        return try {
            commandExecutor.execute {
                val result = runCatching {
                    activeGateway.endTemporaryChat(current.conversationId, current.branchId)
                }
                mainHandler.post {
                    val latest = conversationState
                    if (latest.conversationId != current.conversationId ||
                        latest.branchId != current.branchId ||
                        !latest.transitionInProgress
                    ) {
                        return@post
                    }

                    result.fold(
                        onSuccess = { ended ->
                            if (BackendRuntimePolicies.temporaryEndMatches(
                                    ended = ended,
                                    expectedRuntimeSessionId = ready.runtimeContract.runtimeSessionId,
                                    conversationId = current.conversationId,
                                    branchId = current.branchId,
                                )
                            ) {
                                conversationState = newDurableConversation()
                                onEnded(true)
                            } else {
                                conversationState = current.copy(
                                    transitionInProgress = false,
                                    lastErrorCode = "temporary_chat_end_contract_mismatch",
                                )
                                onEnded(false)
                            }
                        },
                        onFailure = { failure ->
                            conversationState = current.copy(
                                transitionInProgress = false,
                                lastErrorCode = stableFailureCode(failure),
                            )
                            onEnded(false)
                        },
                    )
                }
            }
            true
        } catch (failure: Throwable) {
            conversationState = current.copy(
                transitionInProgress = false,
                lastErrorCode = stableFailureCode(failure),
            )
            onEnded(false)
            false
        }
    }

    fun sendMessage(text: String): Boolean {
        val ready = state as? State.Ready ?: return false
        val current = conversationState
        val normalized = text.trim()
        if (!ready.readiness.inferenceReady || current.busy || normalized.isEmpty()) return false
        val activeGateway = gateway.get() ?: return false

        val operationId = UUID.randomUUID().toString()
        val userMessage = ChatMessage(
            id = UUID.randomUUID().toString(),
            role = ChatRole.USER,
            text = normalized,
        )
        val command = runCatching {
            ProtocolJsonCodec.encodeCommand(
                CommandEnvelopeV2(
                    protocolVersion = ProtocolVersion.CURRENT,
                    commandId = UUID.randomUUID().toString(),
                    requestId = UUID.randomUUID().toString(),
                    commandType = "chat.send_message",
                    submittedAt = Instant.now(),
                    operationId = operationId,
                    correlationId = UUID.randomUUID().toString(),
                    idempotencyKey = UUID.randomUUID().toString(),
                    scope = CommandScope(
                        conversationId = current.conversationId,
                        branchId = current.branchId,
                    ),
                    payloadJson = JSONObject()
                        .put("text", normalized)
                        .toString(),
                ),
            )
        }.getOrElse { failure ->
            conversationState = current.copy(lastErrorCode = stableFailureCode(failure))
            return false
        }

        conversationState = current.copy(
            messages = current.messages + userMessage,
            streamingAssistant = "",
            activeOperationId = operationId,
            lastErrorCode = null,
        )

        return try {
            commandExecutor.execute {
                val result = runCatching {
                    ProtocolJsonCodec.decodeAcknowledgement(
                        activeGateway.submitCommand(command),
                    )
                }
                mainHandler.post {
                    val latest = conversationState
                    if (latest.conversationId != current.conversationId ||
                        latest.branchId != current.branchId ||
                        latest.activeOperationId != operationId
                    ) {
                        return@post
                    }

                    result.fold(
                        onSuccess = { acknowledgement ->
                            if (acknowledgement.status != AcknowledgementStatus.ACCEPTED ||
                                acknowledgement.operationId != operationId
                            ) {
                                conversationState = BackendRuntimePolicies.rollbackRejectedSubmission(
                                    state = latest,
                                    optimisticMessageId = userMessage.id,
                                    errorCode = acknowledgement.rejectionReason
                                        ?: "chat_submission_rejected",
                                )
                            }
                        },
                        onFailure = { failure ->
                            conversationState = BackendRuntimePolicies.rollbackRejectedSubmission(
                                state = latest,
                                optimisticMessageId = userMessage.id,
                                errorCode = stableFailureCode(failure),
                            )
                        },
                    )
                }
            }
            true
        } catch (failure: Throwable) {
            conversationState = current.copy(lastErrorCode = stableFailureCode(failure))
            false
        }
    }

    fun shutdown() {
        running.set(false)
        if (!started.get()) {
            publish(State.Stopped)
            return
        }

        // Bootstrap owns cleanup until it has handed the gateway to workers.
        if (!bootstrapActive.get() && activeWorkers.get() == 0) {
            gateway.getAndSet(null)?.runCatching { close() }
            eventSequenceTracker.reset()
            started.set(false)
            publish(State.Stopped)
        }
    }

    private fun launchWorkers(
        appContext: Context,
        activeGateway: RustNativeGateway,
    ) {
        activeWorkers.set(2)

        platformExecutor.execute {
            runWorker(activeGateway) {
                val processor = AndroidPlatformRequestProcessor(appContext, activeGateway)
                while (isActive(activeGateway)) {
                    var batch = processor.processOnce(
                        maximumRequests = PLATFORM_BATCH_SIZE,
                        timeoutMilliseconds = PLATFORM_LONG_POLL_MILLISECONDS,
                    )
                    while (isActive(activeGateway) && batch.hasMore) {
                        batch = processor.processOnce(
                            maximumRequests = PLATFORM_BATCH_SIZE,
                            timeoutMilliseconds = 0,
                        )
                    }
                }
            }
        }

        eventExecutor.execute {
            runWorker(activeGateway) {
                while (isActive(activeGateway)) {
                    var batch = drainEventBatch(
                        activeGateway = activeGateway,
                        maximumEvents = EVENT_BATCH_SIZE,
                        timeoutMilliseconds = EVENT_LONG_POLL_MILLISECONDS,
                    )
                    dispatch(batch)
                    while (isActive(activeGateway) && batch.hasMore) {
                        batch = drainEventBatch(
                            activeGateway = activeGateway,
                            maximumEvents = EVENT_BATCH_SIZE,
                            timeoutMilliseconds = 0,
                        )
                        dispatch(batch)
                    }
                }
            }
        }
    }

    private inline fun runWorker(
        activeGateway: RustNativeGateway,
        work: () -> Unit,
    ) {
        try {
            work()
        } catch (failure: Throwable) {
            if (running.get()) {
                terminalFailure.compareAndSet(null, stableFailureCode(failure))
                running.set(false)
            }
        } finally {
            if (activeWorkers.decrementAndGet() == 0) {
                gateway.compareAndSet(activeGateway, null)
                activeGateway.runCatching { close() }
                eventSequenceTracker.reset()
                started.set(false)
                val failure = terminalFailure.get()
                publish(if (failure == null) State.Stopped else State.Failed(failure))
            }
        }
    }

    private fun isActive(activeGateway: RustNativeGateway): Boolean =
        running.get() && gateway.get() === activeGateway

    private fun drainEventBatch(
        activeGateway: RustNativeGateway,
        maximumEvents: Int,
        timeoutMilliseconds: Long,
    ): RuntimeEventBatch {
        val decoded = ProtocolJsonCodec.decodeEventBatch(
            activeGateway.drainEvents(maximumEvents, timeoutMilliseconds),
        )
        val sequenceValidation = eventSequenceTracker.accept(decoded)
        return RuntimeEventBatch(
            runtimeSessionId = decoded.runtimeSessionId,
            events = decoded.events,
            hasMore = decoded.hasMore,
            sequenceGapCount = sequenceValidation.gaps.size,
            runtimeSessionChanged = sequenceValidation.runtimeSessionChanged,
        )
    }

    private fun dispatch(batch: RuntimeEventBatch) {
        if (batch.events.isEmpty() &&
            batch.sequenceGapCount == 0 &&
            !batch.runtimeSessionChanged
        ) {
            return
        }

        val lastType = batch.events.lastOrNull()?.eventType
        mainHandler.post {
            eventStatus = eventStatus.copy(
                runtimeSessionId = batch.runtimeSessionId,
                deliveredEvents = eventStatus.deliveredEvents + batch.events.size,
                lastEventType = lastType ?: eventStatus.lastEventType,
                detectedSequenceGaps = eventStatus.detectedSequenceGaps + batch.sequenceGapCount,
                runtimeSessionChanges = eventStatus.runtimeSessionChanges +
                    if (batch.runtimeSessionChanged) 1 else 0,
            )
            batch.events.forEach(::applyConversationEvent)
            eventListeners.forEach { listener ->
                runCatching { listener.onEvents(batch) }
            }
        }
    }

    private fun applyConversationEvent(event: EventEnvelopeV2) {
        val current = conversationState
        val operationId = current.activeOperationId ?: return
        if (event.operationId != operationId) return

        when (event.eventType) {
            // The current runtime emits deltas before it knows whether an inference
            // attempt is a user-visible final answer or an internal tool-call turn.
            // Do not project those bytes into assistant UI until the protocol exposes
            // an explicitly sanitized/final-only streaming capability.
            "chat.token.delta" -> Unit

            "chat.generation.completed" -> {
                val payload = eventPayload(event)
                val completedContent = if (payload.has("content") && !payload.isNull("content")) {
                    payload.optString("content").takeIf { it.isNotEmpty() }
                } else {
                    null
                }
                val finalContent = completedContent.orEmpty()
                val completedMessages = if (finalContent.isEmpty()) {
                    current.messages
                } else {
                    current.messages + ChatMessage(
                        id = UUID.randomUUID().toString(),
                        role = ChatRole.ASSISTANT,
                        text = finalContent,
                    )
                }
                conversationState = current.copy(
                    messages = completedMessages,
                    streamingAssistant = "",
                    activeOperationId = null,
                    lastErrorCode = null,
                )
            }

            "operation.failed" -> {
                val code = eventPayload(event)
                    .optString("code")
                    .takeIf { it.isNotBlank() }
                    ?: "chat_generation_failed"
                conversationState = current.copy(
                    streamingAssistant = "",
                    activeOperationId = null,
                    lastErrorCode = code,
                )
            }

            "operation.cancelled" -> {
                conversationState = current.copy(
                    streamingAssistant = "",
                    activeOperationId = null,
                    lastErrorCode = "chat_generation_cancelled",
                )
            }
        }
    }

    private fun eventPayload(event: EventEnvelopeV2): JSONObject =
        runCatching { JSONObject(event.payloadJson) }
            .getOrElse { JSONObject() }

    private fun initializeEnvelope(configPath: File): ByteArray =
        ProtocolJsonCodec.encodeCommand(
            CommandEnvelopeV2(
                protocolVersion = ProtocolVersion.CURRENT,
                commandId = UUID.randomUUID().toString(),
                requestId = UUID.randomUUID().toString(),
                commandType = "app.initialize",
                submittedAt = Instant.now(),
                correlationId = UUID.randomUUID().toString(),
                payloadJson = JSONObject()
                    .put("config_path", configPath.absolutePath)
                    .toString(),
            ),
        )

    private fun newDurableConversation(): ConversationState = ConversationState(
        conversationId = UUID.randomUUID().toString(),
        branchId = UUID.randomUUID().toString(),
        temporary = false,
        ragEnabled = true,
    )

    private fun resetEventStatus() {
        mainHandler.post { eventStatus = EventStatus() }
    }

    private fun stableFailureCode(failure: Throwable): String {
        val value = failure.message.orEmpty().trim()
        return value.takeIf {
            it.isNotEmpty() &&
                it.length <= 96 &&
                it.all { character ->
                    character.isLetterOrDigit() ||
                        character == '_' ||
                        character == '-' ||
                        character == '.'
                }
        } ?: "backend_runtime_failed"
    }

    private fun publish(value: State) {
        mainHandler.post {
            state = value
            if (value !is State.Ready) {
                conversationState = newDurableConversation()
            }
        }
    }

    private const val PLATFORM_BATCH_SIZE = 8
    private const val PLATFORM_LONG_POLL_MILLISECONDS = 1_000L
    private const val EVENT_BATCH_SIZE = 64
    private const val EVENT_LONG_POLL_MILLISECONDS = 1_000L
    private const val CAP_TEMPORARY_CHAT_SESSIONS = "temporary_chat_sessions"
    private const val CAP_TEMPORARY_CHAT_RAG_DISABLED = "temporary_chat_rag_disabled"
}
