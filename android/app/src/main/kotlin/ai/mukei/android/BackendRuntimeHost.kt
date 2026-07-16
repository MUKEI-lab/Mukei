package ai.mukei.android

import android.content.Context
import android.os.Handler
import android.os.Looper
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
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

/** Process-scoped owner for the native runtime. */
object BackendRuntimeHost {
    sealed interface State {
        data object Starting : State
        data class Ready(val securitySummary: String) : State
        data class Failed(val code: String) : State
        data object Stopped : State
    }

    private val started = AtomicBoolean(false)
    private val mainHandler = Handler(Looper.getMainLooper())
    private val executor = Executors.newSingleThreadExecutor { runnable ->
        Thread(runnable, "mukei-secure-bootstrap").apply { isDaemon = true }
    }
    private val gateway = AtomicReference<RustNativeGateway?>(null)

    var state: State by mutableStateOf<State>(State.Starting)
        private set

    fun start(context: Context) {
        if (!started.compareAndSet(false, true)) return
        val appContext = context.applicationContext
        executor.execute {
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

                val nativeGateway = SecureRuntimeFactory.open(appContext, configJson)
                val acknowledgement = JSONObject(
                    String(
                        nativeGateway.submitCommand(initializeEnvelope(configPath)),
                        StandardCharsets.UTF_8,
                    ),
                )
                if (acknowledgement.optString("status") != "accepted") {
                    val reason = acknowledgement.optString("rejection_reason", "initialize_rejected")
                    nativeGateway.close()
                    throw IllegalStateException(reason)
                }
                gateway.set(nativeGateway)
                val security = JSONObject(
                    String(nativeGateway.securityStatus(), StandardCharsets.UTF_8),
                )
                val summary = listOf(
                    security.optString("sqlcipher", "unknown"),
                    if (security.optBoolean("panic_hook", false)) "panic-contained" else "panic-hook-missing",
                ).joinToString(" · ")
                publish(State.Ready(summary))
            } catch (failure: Throwable) {
                gateway.getAndSet(null)?.runCatching { close() }
                publish(State.Failed(stableFailureCode(failure)))
            }
        }
    }

    fun shutdown() {
        val nativeGateway = gateway.getAndSet(null)
        executor.execute {
            nativeGateway?.runCatching { close() }
            publish(State.Stopped)
        }
    }

    private fun initializeEnvelope(configPath: File): ByteArray {
        val commandId = UUID.randomUUID().toString()
        return JSONObject()
            .put("protocol_version", JSONObject().put("major", 2).put("minor", 0))
            .put("command_id", commandId)
            .put("request_id", UUID.randomUUID().toString())
            .put("command_type", "app.initialize")
            .put("submitted_at", Instant.now().toString())
            .put("correlation_id", UUID.randomUUID().toString())
            .put("payload", JSONObject().put("config_path", configPath.absolutePath))
            .toString()
            .toByteArray(StandardCharsets.UTF_8)
    }

    private fun stableFailureCode(failure: Throwable): String {
        val value = failure.message.orEmpty().trim()
        return value.takeIf {
            it.isNotEmpty() && it.length <= 96 && it.all { character ->
                character.isLetterOrDigit() || character == '_' || character == '-' || character == '.'
            }
        } ?: "backend_boot_failed"
    }

    private fun publish(value: State) {
        mainHandler.post { state = value }
    }
}
