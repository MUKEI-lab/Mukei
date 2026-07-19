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
    private val activeCommandCalls = AtomicInteger(0)
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
        Thread(runnable, "mukei-chat-command").apply { isDaemon = true }
    }
    private val gateway = AtomicReference<RustNativeGateway?>(null)
    private val eventListeners = CopyOnWriteArraySet<EventBatchListener>()
    private val eventSequenceTracker = EventSequenceTracker()

    var state: State by mutableStateOf<State>(State.Starting)
        private set

    var eventStatus: EventStatus by mutableStateOf(EventStatus())
        private set

    var chatState: ChatUiState by mutableStateOf(ChatUiState())
        private set

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
        resetChatState()
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
                    if (!running.get()) {
                        return@execute
                    }

                    gateway.set(activeGateway)
                    val runtimeContract = RuntimeContractJsonCodec.decode(
                        activeGateway.protocolCapabilities(),
                    )
                    val readiness = AppReadiness.fromSecurityStatus(activeGateway.securityStatus())
                    if (!readiness.shellUsable) {
                        throw IllegalStateException("secure_storage_not_ready")
                    }
                    if (!running.get()) {
                        return@execute
                    }

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

    fun shutdown() {
        running.set(false)
        if (!started.get()) {
            publish(State.Stopped)
            return
        }

        // Never close a gateway concurrently with bootstrap, long-poll workers, or one
        // of the bounded begin/end/submit JNI calls owned by commandExecutor.
        if (!bootstrapActive.get()) {
            gateway.get()?.let(::finishGatewayIfIdle)
        }
    }

    /** Start a blank durable chat. A Temporary Chat is purged before the switch completes. */
    fun startNewNormalChat() = onMain {
        if (chatState.transitionInProgress) return@onMain
        val current = chatState.session
        if (current?.temporary == true) {
            endTemporaryChatThen { chatState = ChatUiState() }
            return@onMain
        }
        if (chatState.generationInProgress) {
            chatState = chatState.copy(errorCode = "generation_in_progress")
            return@onMain
        }
        chatState = ChatUiState()
    }

    /** Begin a runtime-minted Temporary Chat. RAG, files and web tools are unavailable. */
    fun beginTemporaryChat() = onMain {
        if (chatState.transitionInProgress || chatState.generationInProgress) {
            chatState = chatState.copy(errorCode = "chat_transition_busy")
            return@onMain
        }
        if (chatState.temporary) return@onMain

        val ready = state as? State.Ready
        if (ready == null || !ready.readiness.inferenceReady) {
            chatState = chatState.copy(errorCode = "inference_not_ready")
            return@onMain
        }
        val capabilities = ready.runtimeContract.protocol.capabilities.toSet()
        if (CAP_TEMPORARY_CHAT_SESSIONS !in capabilities ||
            CAP_TEMPORARY_CHAT_RAG_DISABLED !in capabilities
        ) {
            chatState = chatState.copy(errorCode = "temporary_chat_unavailable")
            return@onMain
        }
        val activeGateway = gateway.get()
        if (activeGateway == null || !isActive(activeGateway)) {
            chatState = chatState.copy(errorCode = "backend_runtime_unavailable")
            return@onMain
        }

        chatState = chatState.copy(transitionInProgress = true, errorCode = null)
        executeGatewayCall(activeGateway) {
            val result = activeGateway.beginTemporaryChat()
            mainHandler.post {
                if (gateway.get() !== activeGateway || !running.get()) return@post
                chatState = ChatUiState(
                    session = ActiveChatScope(
                        conversationId = result.conversationId,
                        branchId = result.branchId,
                        kind = ChatSessionKind.TEMPORARY,
                    ),
                )
            }
        }
    }

    /**
     * Purge Temporary Chat before an external navigation transition. The continuation runs
     * only after native end confirms the process-local session was removed.
     */
    fun leaveTemporaryChat(onComplete: () -> Unit) = onMain {
        if (chatState.transitionInProgress) return@onMain
        if (!chatState.temporary) {
            onComplete()
            return@onMain
        }
        endTemporaryChatThen(onComplete)
    }

    /** Submit one message through Protocol V2 using the active durable/temporary scope. */
    fun submitChatMessage(rawText: String) = onMain {
        val text = rawText.trim()
        if (text.isEmpty()) return@onMain
        if (text.toByteArray(StandardCharsets.UTF_8).size > MAX_CHAT_TEXT_BYTES) {
            chatState = chatState.copy(errorCode = "chat_message_too_large")
            return@onMain
        }
        if (chatState.generationInProgress || chatState.transitionInProgress) {
            chatState = chatState.copy(errorCode = "generation_in_progress")
            return@onMain
        }
        val ready = state as? State.Ready
        if (ready == null || !ready.readiness.inferenceReady) {
            chatState = chatState.copy(errorCode = "inference_not_ready")
            return@onMain
        }
        val activeGateway = gateway.get()
        if (activeGateway == null || !isActive(activeGateway)) {
            chatState = chatState.copy(errorCode = "backend_runtime_unavailable")
            return@onMain
        }

        val session = chatState.session ?: ActiveChatScope(
            conversationId = UUID.randomUUID().toString(),
            branchId = UUID.randomUUID().toString(),
            kind = ChatSessionKind.NORMAL,
        )
        val correlationId = UUID.randomUUID().toString()
        val userMessageId = "user:$correlationId"
        val command = CommandEnvelopeV2(
            protocolVersion = ProtocolVersion.CURRENT,
            commandId = UUID.randomUUID().toString(),
            requestId = UUID.randomUUID().toString(),
            commandType = "chat.send_message",
            submittedAt = Instant.now(),
            correlationId = correlationId,
            idempotencyKey = UUID.randomUUID().toString(),
            scope = CommandScope(
                conversationId = session.conversationId,
                branchId = session.branchId,
            ),
            payloadJson = JSONObject().put("text", text).toString(),
        )
        chatState = chatState.copy(
            session = session,
            messages = chatState.messages + ChatUiMessage(
                id = userMessageId,
                role = ChatMessageRole.USER,
                text = text,
            ),
            generationInProgress = true,
            activeCorrelationId = correlationId,
            activeOperationId = null,
            errorCode = null,
        )

        executeGatewayCall(activeGateway) {
            val acknowledgement = ProtocolJsonCodec.decodeAcknowledgement(
                activeGateway.submitCommand(ProtocolJsonCodec.encodeCommand(command)),
            )
            mainHandler.post {
                if (gateway.get() !== activeGateway || chatState.activeCorrelationId != correlationId) {
                    return@post
                }
                if (acknowledgement.status == AcknowledgementStatus.ACCEPTED &&
                    !acknowledgement.operationId.isNullOrBlank()
                ) {
                    chatState = chatState.copy(activeOperationId = acknowledgement.operationId)
                } else {
                    chatState = chatState.copy(
                        messages = chatState.messages.filterNot { it.id == userMessageId },
                        generationInProgress = false,
                        activeCorrelationId = null,
                        activeOperationId = null,
                        errorCode = acknowledgement.rejectionReason ?: "chat_command_rejected",
                    )
                }
            }
        }
    }

    fun clearChatError() = onMain {
        if (chatState.errorCode != null) chatState = chatState.copy(errorCode = null)
    }

    private fun endTemporaryChatThen(onComplete: () -> Unit) {
        val session = chatState.session
        if (session?.temporary != true) {
            onComplete()
            return
        }
        val activeGateway = gateway.get()
        if (activeGateway == null || !isActive(activeGateway)) {
            chatState = chatState.copy(errorCode = "backend_runtime_unavailable")
            return
        }
        chatState = chatState.copy(transitionInProgress = true, errorCode = null)
        executeGatewayCall(activeGateway) {
            val result = activeGateway.endTemporaryChat(session.conversationId, session.branchId)
            mainHandler.post {
                if (gateway.get() !== activeGateway) return@post
                val current = chatState.session
                if (result.ended && current?.conversationId == session.conversationId &&
                    current.branchId == session.branchId
                ) {
                    chatState = ChatUiState()
                    onComplete()
                } else if (current?.conversationId == session.conversationId) {
                    chatState = chatState.copy(
                        transitionInProgress = false,
                        errorCode = "temporary_chat_end_failed",
                    )
                }
            }
        }
    }

    private fun executeGatewayCall(
        activeGateway: RustNativeGateway,
        call: () -> Unit,
    ) {
        activeCommandCalls.incrementAndGet()
        try {
            commandExecutor.execute {
                try {
                    if (isActive(activeGateway)) call()
                } catch (failure: Throwable) {
                    val code = stableFailureCode(failure)
                    mainHandler.post {
                        if (gateway.get() === activeGateway) {
                            val correlation = chatState.activeCorrelationId
                            chatState = chatState.copy(
                                messages = if (correlation == null) {
                                    chatState.messages
                                } else {
                                    chatState.messages.filterNot { it.id == "user:$correlation" }
                                },
                                generationInProgress = false,
                                transitionInProgress = false,
                                activeCorrelationId = null,
                                activeOperationId = null,
                                errorCode = code,
                            )
                        }
                    }
                } finally {
                    activeCommandCalls.decrementAndGet()
                    finishGatewayIfIdle(activeGateway)
                }
            }
        } catch (failure: Throwable) {
            activeCommandCalls.decrementAndGet()
            mainHandler.post {
                chatState = chatState.copy(
                    generationInProgress = false,
                    transitionInProgress = false,
                    errorCode = stableFailureCode(failure),
                )
            }
            finishGatewayIfIdle(activeGateway)
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
                        activeGateway,
                        EVENT_BATCH_SIZE,
                        EVENT_LONG_POLL_MILLISECONDS,
                    )
                    dispatch(batch)
                    while (isActive(activeGateway) && batch.hasMore) {
                        batch = drainEventBatch(activeGateway, EVENT_BATCH_SIZE, 0)
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
            activeWorkers.decrementAndGet()
            finishGatewayIfIdle(activeGateway)
        }
    }

    private fun finishGatewayIfIdle(activeGateway: RustNativeGateway) {
        if (running.get() || bootstrapActive.get()) return
        if (activeWorkers.get() != 0 || activeCommandCalls.get() != 0) return
        if (!gateway.compareAndSet(activeGateway, null)) return

        activeGateway.runCatching { close() }
        eventSequenceTracker.reset()
        started.set(false)
        val failure = terminalFailure.get()
        mainHandler.post {
            chatState = ChatUiState(errorCode = failure)
        }
        publish(if (failure == null) State.Stopped else State.Failed(failure))
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
        if (batch.events.isEmpty() && batch.sequenceGapCount == 0 && !batch.runtimeSessionChanged) return
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
            chatState = if (batch.runtimeSessionChanged) {
                ChatUiState(errorCode = "runtime_session_changed")
            } else {
                ChatEventReducer.reduce(chatState, batch.events)
            }
            eventListeners.forEach { listener ->
                runCatching { listener.onEvents(batch) }
            }
        }
    }

    private fun initializeEnvelope(configPath: File): ByteArray = ProtocolJsonCodec.encodeCommand(
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

    private fun resetEventStatus() {
        mainHandler.post { eventStatus = EventStatus() }
    }

    private fun resetChatState() {
        mainHandler.post { chatState = ChatUiState() }
    }

    private inline fun onMain(crossinline action: () -> Unit) {
        if (Looper.myLooper() == Looper.getMainLooper()) {
            action()
        } else {
            mainHandler.post { action() }
        }
    }

    private fun stableFailureCode(failure: Throwable): String {
        val value = failure.message.orEmpty().trim()
        return value.takeIf {
            it.isNotEmpty() && it.length <= 96 && it.all { character ->
                character.isLetterOrDigit() || character == '_' || character == '-' || character == '.'
            }
        } ?: "backend_runtime_failed"
    }

    private fun publish(value: State) {
        mainHandler.post { state = value }
    }

    private const val CAP_TEMPORARY_CHAT_SESSIONS = "temporary_chat_sessions"
    private const val CAP_TEMPORARY_CHAT_RAG_DISABLED = "temporary_chat_rag_disabled"
    private const val MAX_CHAT_TEXT_BYTES = 32 * 1024
    private const val PLATFORM_BATCH_SIZE = 8
    private const val PLATFORM_LONG_POLL_MILLISECONDS = 1_000L
    private const val EVENT_BATCH_SIZE = 64
    private const val EVENT_LONG_POLL_MILLISECONDS = 1_000L
}
