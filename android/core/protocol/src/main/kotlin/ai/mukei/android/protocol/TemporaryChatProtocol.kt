package ai.mukei.android.protocol

import java.nio.charset.StandardCharsets
import org.json.JSONObject

/** Runtime-minted, process-local Temporary Chat scope. */
data class TemporaryChatSessionV2(
    val protocolVersion: ProtocolVersion,
    val runtimeSessionId: String,
    val conversationId: String,
    val branchId: String,
    val ragEnabled: Boolean,
)

/** Result of explicitly ending and purging a Temporary Chat session. */
data class TemporaryChatEndV2(
    val protocolVersion: ProtocolVersion,
    val runtimeSessionId: String,
    val conversationId: String,
    val branchId: String,
    val ended: Boolean,
    val ragEnabled: Boolean,
)

/** Typed codec for the dedicated Temporary Chat JNI calls. */
object TemporaryChatJsonCodec {
    fun decodeSession(bytes: ByteArray): TemporaryChatSessionV2 {
        val root = parseRoot(bytes)
        requireNoNativeError(root)
        if (!root.optBoolean("temporary", false)) {
            throw ProtocolCodecException("invalid_temporary_chat_session")
        }
        val value = TemporaryChatSessionV2(
            protocolVersion = decodeVersion(root),
            runtimeSessionId = requiredId(root, "runtime_session_id"),
            conversationId = requiredId(root, "conversation_id"),
            branchId = requiredId(root, "branch_id"),
            ragEnabled = root.optBoolean("rag_enabled", true),
        )
        if (value.ragEnabled) {
            throw ProtocolCodecException("temporary_chat_rag_must_be_disabled")
        }
        return value
    }

    fun decodeEnd(bytes: ByteArray): TemporaryChatEndV2 {
        val root = parseRoot(bytes)
        requireNoNativeError(root)
        if (!root.optBoolean("temporary", false)) {
            throw ProtocolCodecException("invalid_temporary_chat_end")
        }
        val value = TemporaryChatEndV2(
            protocolVersion = decodeVersion(root),
            runtimeSessionId = requiredId(root, "runtime_session_id"),
            conversationId = requiredId(root, "conversation_id"),
            branchId = requiredId(root, "branch_id"),
            ended = root.optBoolean("ended", false),
            ragEnabled = root.optBoolean("rag_enabled", true),
        )
        if (value.ragEnabled) {
            throw ProtocolCodecException("temporary_chat_rag_must_be_disabled")
        }
        return value
    }

    private fun decodeVersion(root: JSONObject): ProtocolVersion {
        val version = root.optJSONObject("protocol_version")
            ?: throw ProtocolCodecException("missing_protocol_version")
        return ProtocolVersion(
            major = version.optInt("major", -1),
            minor = version.optInt("minor", -1),
        ).also {
            if (it.major < 0 || it.minor < 0 || !it.isCompatible()) {
                throw ProtocolCodecException("unsupported_protocol")
            }
        }
    }

    private fun parseRoot(bytes: ByteArray): JSONObject = try {
        JSONObject(String(bytes, StandardCharsets.UTF_8))
    } catch (_: Throwable) {
        throw ProtocolCodecException("invalid_temporary_chat_json")
    }

    private fun requireNoNativeError(root: JSONObject) {
        root.optJSONObject("error")?.let { error ->
            throw ProtocolCodecException(error.optString("code", "temporary_chat_native_error"))
        }
    }

    private fun requiredId(root: JSONObject, key: String): String {
        val value = root.optString(key).takeIf { it.isNotBlank() }
            ?: throw ProtocolCodecException("missing_$key")
        val valid = value.length <= ProtocolLimits.MAX_PROTOCOL_ID_LENGTH &&
            value == value.trim() &&
            value.all { character ->
                character.isLetterOrDigit() || character == '-' || character == '_' ||
                    character == '.' || character == ':' || character == '/'
            }
        if (!valid) throw ProtocolCodecException("invalid_$key")
        return value
    }
}
