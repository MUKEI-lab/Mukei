package ai.mukei.android

import java.nio.charset.StandardCharsets
import org.json.JSONObject

enum class ReadinessStatus {
    READY,
    ACTION_REQUIRED,
    DEGRADED,
    UNAVAILABLE,
    UNKNOWN,
}

data class ReadinessDimension(
    val status: ReadinessStatus,
    val code: String? = null,
)

data class AppReadiness(
    val secureRuntime: ReadinessDimension,
    val encryptedDatabase: ReadinessDimension,
    val encryptedProjections: ReadinessDimension,
    val inference: ReadinessDimension,
    val panicContainment: ReadinessDimension,
) {
    /** Product shell can open even when inference still needs model artifacts. */
    val shellUsable: Boolean
        get() = secureRuntime.status == ReadinessStatus.READY &&
            encryptedDatabase.status == ReadinessStatus.READY &&
            encryptedProjections.status == ReadinessStatus.READY

    val inferenceReady: Boolean
        get() = inference.status == ReadinessStatus.READY

    fun diagnosticSummary(): String = buildList {
        add("runtime:${secureRuntime.status.machineTag()}")
        add("database:${encryptedDatabase.status.machineTag()}")
        add("projections:${encryptedProjections.status.machineTag()}")
        add("inference:${inference.status.machineTag()}")
        add("panic:${panicContainment.status.machineTag()}")
    }.joinToString(" · ")

    companion object {
        fun fromSecurityStatus(payload: ByteArray): AppReadiness {
            val root = try {
                JSONObject(String(payload, StandardCharsets.UTF_8))
            } catch (_: Throwable) {
                throw IllegalStateException("security_status_invalid")
            }
            root.optJSONObject("error")?.let { error ->
                throw IllegalStateException(error.optString("code", "security_status_failed"))
            }

            val database = when (root.optString("sqlcipher", "unknown")) {
                "encrypted" -> ReadinessDimension(ReadinessStatus.READY, "encrypted")
                "not_configured" -> ReadinessDimension(ReadinessStatus.UNAVAILABLE, "not_configured")
                else -> ReadinessDimension(ReadinessStatus.UNKNOWN, "sqlcipher_unknown")
            }
            val projections = when (root.optString("projections", "unknown")) {
                "encrypted" -> ReadinessDimension(ReadinessStatus.READY, "encrypted")
                "unavailable", "not_configured" ->
                    ReadinessDimension(ReadinessStatus.UNAVAILABLE, "projections_unavailable")
                else -> ReadinessDimension(ReadinessStatus.UNKNOWN, "projections_unknown")
            }
            val inference = when (root.optString("rag", "unknown")) {
                "ready" -> ReadinessDimension(ReadinessStatus.READY, "ready")
                "artifacts_required" ->
                    ReadinessDimension(ReadinessStatus.ACTION_REQUIRED, "artifacts_required")
                "unavailable" -> ReadinessDimension(ReadinessStatus.UNAVAILABLE, "unavailable")
                else -> ReadinessDimension(ReadinessStatus.UNKNOWN, "inference_unknown")
            }
            val panic = if (root.optBoolean("panic_hook", false)) {
                ReadinessDimension(ReadinessStatus.READY, "panic_contained")
            } else {
                ReadinessDimension(ReadinessStatus.DEGRADED, "panic_hook_missing")
            }

            return AppReadiness(
                secureRuntime = ReadinessDimension(ReadinessStatus.READY, "ready"),
                encryptedDatabase = database,
                encryptedProjections = projections,
                inference = inference,
                panicContainment = panic,
            )
        }
    }
}

private fun ReadinessStatus.machineTag(): String = name.lowercase()
