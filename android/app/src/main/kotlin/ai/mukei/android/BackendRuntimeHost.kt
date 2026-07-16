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

/** Process-scoped owner for the native runtime, event stream and Android broker. */
object BackendRuntimeHost {
    sealed interface State {
        data object Starting : State
        data class Ready(val securitySummary: String) : State
        data class Failed(val code: String) : State
        data object Stopped : State
    }

    data class RuntimeEventBatch(
        val events: List<String>,
        val hasMore: Boolean,
    )

    data class EventStatus(
        val deliveredEvents: Long = 0,
        val lastEventType: String? = null,
    )

    fun interface EventBatchListener {
        fun onEvents(batch: RuntimeEventBatch)
    }

    private val started = AtomicBoolean(false)
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
    private val gateway = AtomicReference<RustNativeGateway?>(null)
    private val eventListeners = CopyOnWriteArraySet<EventBatchListener>()

    var state: State by mutableStateOf<State>(State.Starting)
        private set

    var eventStatus: EventStatus by mutableStateOf(EventStatus())
        private set

    fun addEventListener(listener: EventBatchListener): Closeable {
        eventListeners += listener
        return Closeable { eventListeners -= listener }
    }

    fun start(context: Context) {
        if (!started.compareAndSet(false, true)) return
        running.set(true)
        terminalFailure.set(null)
        val appContext = context.applicationContext
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
                val acknowledgement = JSONObject(
                    String(
                        activeGateway.submitCommand(initializeEnvelope(configPath)),
                        StandardCharsets.UTF_8,
                    ),
                )
                if (acknowledgement.optString("status") != "accepted") {
                    throw IllegalStateException(
                        acknowledgement.optString("rejection_reason", "initialize_rejected"),
                    )
                }
                if (!running.get()) {
                    publish(State.Stopped)
                    return@execute
                }

                gateway.set(activeGateway)
                val security = JSONObject(
                    String(activeGateway.securityStatus(), StandardCharsets.UTF_8),
                )
                val summary = listOf(
                    security.optString("sqlcipher", "unknown"),
                    security.optString("projections", "unknown"),
                    security.optString("object_store", "unknown"),
                    security.optString("rag", "unknown"),
                    if (security.optBoolean("panic_hook", false)) {
                        "panic-contained"
                    } else {
                        "panic-hook-missing"
                    },
                ).joinToString(" · ")
                publish(State.Ready(summary))

                workersLaunched = true
                launchWorkers(appContext, activeGateway)
            } catch (failure: Throwable) {
                running.set(false)
                terminalFailure.compareAndSet(null, stableFailureCode(failure))
                publish(State.Failed(terminalFailure.get() ?: "backend_runtime_failed"))
            } finally {
                if (!workersLaunched) {
                    ownedGateway?.runCatching { close() }
                }
            }
        }
    }

    fun shutdown() {
        running.set(false)
        if (activeWorkers.get() == 0) {
            gateway.getAndSet(null)?.runCatching { close() }
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
            if (activeWorkers.decrementAndGet() == 0) {
                gateway.compareAndSet(activeGateway, null)
                activeGateway.runCatching { close() }
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
        val payload = JSONObject(
            String(
                activeGateway.drainEvents(maximumEvents, timeoutMilliseconds),
                StandardCharsets.UTF_8,
            ),
        )
        payload.optJSONObject("error")?.let { error ->
            throw IllegalStateException(error.optString("code", "event_drain_failed"))
        }
        val eventsJson = payload.optJSONArray("events")
        val events = buildList {
            if (eventsJson != null) {
                for (index in 0 until eventsJson.length()) {
                    add(eventsJson.getJSONObject(index).toString())
                }
            }
        }
        return RuntimeEventBatch(
            events = events,
            hasMore = payload.optBoolean("has_more", false),
        )
    }

    private fun dispatch(batch: RuntimeEventBatch) {
        if (batch.events.isEmpty()) return
        val lastType = runCatching {
            JSONObject(batch.events.last()).optString("event_type").ifBlank { null }
        }.getOrNull()
        mainHandler.post {
            eventStatus = eventStatus.copy(
                deliveredEvents = eventStatus.deliveredEvents + batch.events.size,
                lastEventType = lastType ?: eventStatus.lastEventType,
            )
            eventListeners.forEach { listener ->
                runCatching { listener.onEvents(batch) }
            }
        }
    }

    private fun initializeEnvelope(configPath: File): ByteArray = JSONObject()
        .put("protocol_version", JSONObject().put("major", 2).put("minor", 0))
        .put("command_id", UUID.randomUUID().toString())
        .put("request_id", UUID.randomUUID().toString())
        .put("command_type", "app.initialize")
        .put("submitted_at", Instant.now().toString())
        .put("correlation_id", UUID.randomUUID().toString())
        .put("payload", JSONObject().put("config_path", configPath.absolutePath))
        .toString()
        .toByteArray(StandardCharsets.UTF_8)

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

    private const val PLATFORM_BATCH_SIZE = 8
    private const val PLATFORM_LONG_POLL_MILLISECONDS = 1_000L
    private const val EVENT_BATCH_SIZE = 64
    private const val EVENT_LONG_POLL_MILLISECONDS = 1_000L
}
