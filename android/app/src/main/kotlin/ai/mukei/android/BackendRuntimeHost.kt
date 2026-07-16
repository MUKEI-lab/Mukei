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
import java.io.File
import java.nio.charset.StandardCharsets
import java.time.Instant
import java.util.UUID
import java.util.concurrent.Executors
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicReference
import org.json.JSONObject

/** Process-scoped owner for the native runtime and Android platform broker. */
object BackendRuntimeHost {
    sealed interface State {
        data object Starting : State
        data class Ready(val securitySummary: String) : State
        data class Failed(val code: String) : State
        data object Stopped : State
    }

    private val started = AtomicBoolean(false)
    private val running = AtomicBoolean(false)
    private val mainHandler = Handler(Looper.getMainLooper())
    private val executor = Executors.newSingleThreadExecutor { runnable ->
        Thread(runnable, "mukei-backend-host").apply { isDaemon = true }
    }
    private val gateway = AtomicReference<RustNativeGateway?>(null)

    var state: State by mutableStateOf<State>(State.Starting)
        private set

    fun start(context: Context) {
        if (!started.compareAndSet(false, true)) return
        running.set(true)
        val appContext = context.applicationContext
        executor.execute {
            var nativeGateway: RustNativeGateway? = null
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

                nativeGateway = SecureRuntimeFactory.open(appContext, configJson)
                val acknowledgement = JSONObject(
                    String(
                        nativeGateway.submitCommand(initializeEnvelope(configPath)),
                        StandardCharsets.UTF_8,
                    ),
                )
                if (acknowledgement.optString("status") != "accepted") {
                    throw IllegalStateException(
                        acknowledgement.optString("rejection_reason", "initialize_rejected"),
                    )
                }
                gateway.set(nativeGateway)
                val security = JSONObject(
                    String(nativeGateway.securityStatus(), StandardCharsets.UTF_8),
                )
                val summary = listOf(
                    security.optString("sqlcipher", "unknown"),
                    security.optString("projections", "unknown"),
                    security.optString("rag", "unknown"),
                    if (security.optBoolean("panic_hook", false)) {
                        "panic-contained"
                    } else {
                        "panic-hook-missing"
                    },
                ).joinToString(" · ")
                publish(State.Ready(summary))

                val processor = AndroidPlatformRequestProcessor(appContext, nativeGateway)
                while (running.get() && gateway.get() === nativeGateway) {
                    var batch = processor.processOnce(
                        maximumRequests = PLATFORM_BATCH_SIZE,
                        timeoutMilliseconds = PLATFORM_LONG_POLL_MILLISECONDS,
                    )
                    while (running.get() && batch.hasMore) {
                        batch = processor.processOnce(
                            maximumRequests = PLATFORM_BATCH_SIZE,
                            timeoutMilliseconds = 0,
                        )
                    }
                }
            } catch (failure: Throwable) {
                if (running.get()) {
                    publish(State.Failed(stableFailureCode(failure)))
                }
            } finally {
                nativeGateway?.let { active ->
                    gateway.compareAndSet(active, null)
                    active.runCatching { close() }
                }
                if (!running.get()) {
                    publish(State.Stopped)
                }
            }
        }
    }

    fun shutdown() {
        running.set(false)
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
}
