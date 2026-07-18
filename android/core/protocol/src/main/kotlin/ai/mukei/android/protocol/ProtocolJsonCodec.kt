package ai.mukei.android.protocol

import java.nio.charset.StandardCharsets
import java.time.Instant
import org.json.JSONArray
import org.json.JSONObject
import org.json.JSONTokener

/** Stable protocol-boundary failure that can be surfaced without leaking raw parser exceptions. */
class ProtocolCodecException(
    val code: String,
) : IllegalArgumentException(code)

/**
 * Central JSON codec for the in-process Protocol V2 JNI transport.
 *
 * Feature/UI code must consume typed models rather than constructing or parsing protocol JSON.
 */
object ProtocolJsonCodec {
    fun encodeCommand(envelope: CommandEnvelopeV2): ByteArray {
        require(envelope.protocolVersion.isCompatible()) { "unsupported_protocol" }
        requireProtocolId(envelope.commandId, "invalid_command_id")
        requireProtocolId(envelope.requestId, "invalid_request_id")
        requireProtocolId(envelope.correlationId, "invalid_correlation_id")
        envelope.operationId?.let { requireProtocolId(it, "invalid_operation_id") }
        envelope.idempotencyKey?.let {
            if (it.isBlank() || it.length > ProtocolLimits.MAX_IDEMPOTENCY_KEY_LENGTH) {
                throw ProtocolCodecException("invalid_idempotency_key")
            }
        }
        if (envelope.commandType.isBlank() || envelope.commandType.length > ProtocolLimits.MAX_COMMAND_TYPE_LENGTH) {
            throw ProtocolCodecException("invalid_command_type")
        }

        val root = JSONObject()
            .put("protocol_version", encodeVersion(envelope.protocolVersion))
            .put("command_id", envelope.commandId)
            .put("request_id", envelope.requestId)
            .put("command_type", envelope.commandType)
            .put("submitted_at", envelope.submittedAt.toString())
            .put("correlation_id", envelope.correlationId)
            .put("payload", parseJsonValue(envelope.payloadJson))

        envelope.operationId?.let { root.put("operation_id", it) }
        envelope.idempotencyKey?.let { root.put("idempotency_key", it) }
        envelope.scope?.let { root.put("scope", encodeScope(it)) }

        return root.toString().toByteArray(StandardCharsets.UTF_8).also { bytes ->
            if (bytes.size > ProtocolLimits.MAX_COMMAND_ENVELOPE_BYTES) {
                throw ProtocolCodecException("command_envelope_too_large")
            }
        }
    }

    fun decodeAcknowledgement(bytes: ByteArray): CommandAcknowledgementV2 {
        val root = parseRoot(bytes, "invalid_acknowledgement_json")
        root.optJSONObject("error")?.let { error ->
            throw ProtocolCodecException(error.optString("code", "native_protocol_error"))
        }

        val status = when (requiredString(root, "status")) {
            "accepted" -> AcknowledgementStatus.ACCEPTED
            "rejected" -> AcknowledgementStatus.REJECTED
            else -> throw ProtocolCodecException("invalid_acknowledgement_status")
        }
        val acknowledgement = CommandAcknowledgementV2(
            protocolVersion = decodeVersion(requiredObject(root, "protocol_version")),
            commandId = requiredString(root, "command_id"),
            requestId = requiredString(root, "request_id"),
            correlationId = requiredString(root, "correlation_id"),
            operationId = optionalString(root, "operation_id"),
            status = status,
            rejectionReason = optionalString(root, "rejection_reason"),
            timestamp = parseInstant(requiredString(root, "timestamp"), "invalid_acknowledgement_timestamp"),
        )
        validateAcknowledgement(acknowledgement)
        return acknowledgement
    }

    fun decodeEventBatch(bytes: ByteArray): EventBatchV2 {
        if (bytes.size > ProtocolLimits.MAX_EVENT_BATCH_BYTES) {
            throw ProtocolCodecException("event_batch_too_large")
        }
        val root = parseRoot(bytes, "invalid_event_batch_json")
        root.optJSONObject("error")?.let { error ->
            throw ProtocolCodecException(error.optString("code", "event_drain_failed"))
        }

        val eventsJson = root.optJSONArray("events") ?: JSONArray()
        if (eventsJson.length() > ProtocolLimits.MAX_EVENT_BATCH_ITEMS) {
            throw ProtocolCodecException("event_batch_item_limit_exceeded")
        }
        val events = buildList(eventsJson.length()) {
            for (index in 0 until eventsJson.length()) {
                add(decodeEvent(eventsJson.getJSONObject(index)))
            }
        }
        val batch = EventBatchV2(
            protocolVersion = decodeVersion(requiredObject(root, "protocol_version")),
            runtimeSessionId = requiredString(root, "runtime_session_id"),
            drainedAt = parseInstant(requiredString(root, "drained_at"), "invalid_event_batch_timestamp"),
            events = events,
            hasMore = root.optBoolean("has_more", false),
        )
        if (!batch.protocolVersion.isCompatible()) {
            throw ProtocolCodecException("unsupported_protocol")
        }
        requireProtocolId(batch.runtimeSessionId, "invalid_runtime_session_id")
        return batch
    }

    private fun decodeEvent(root: JSONObject): EventEnvelopeV2 {
        val event = EventEnvelopeV2(
            protocolVersion = decodeVersion(requiredObject(root, "protocol_version")),
            eventId = requiredString(root, "event_id"),
            streamId = requiredString(root, "stream_id"),
            sequence = root.optLong("sequence", 0L),
            eventType = requiredString(root, "event_type"),
            emittedAt = parseInstant(requiredString(root, "emitted_at"), "invalid_event_timestamp"),
            correlationId = optionalString(root, "correlation_id"),
            operationId = optionalString(root, "operation_id"),
            requestId = optionalString(root, "request_id"),
            commandId = optionalString(root, "command_id"),
            commandType = optionalString(root, "command_type"),
            payloadJson = jsonValueToString(root.opt("payload")),
        )
        validateEvent(event)
        return event
    }

    private fun validateAcknowledgement(value: CommandAcknowledgementV2) {
        if (!value.protocolVersion.isCompatible()) throw ProtocolCodecException("unsupported_protocol")
        requireProtocolId(value.commandId, "invalid_command_id")
        requireProtocolId(value.requestId, "invalid_request_id")
        requireProtocolId(value.correlationId, "invalid_correlation_id")
        value.operationId?.let { requireProtocolId(it, "invalid_operation_id") }
        if (value.status == AcknowledgementStatus.ACCEPTED && value.operationId == null) {
            throw ProtocolCodecException("accepted_ack_missing_operation_id")
        }
        if (value.status == AcknowledgementStatus.REJECTED && value.rejectionReason.isNullOrBlank()) {
            throw ProtocolCodecException("rejected_ack_missing_reason")
        }
    }

    private fun validateEvent(value: EventEnvelopeV2) {
        if (!value.protocolVersion.isCompatible()) throw ProtocolCodecException("unsupported_protocol")
        requireProtocolId(value.eventId, "invalid_event_id")
        requireProtocolId(value.streamId, "invalid_stream_id")
        if (value.sequence <= 0L) throw ProtocolCodecException("invalid_event_sequence")
        if (value.eventType.isBlank() || value.eventType.length > ProtocolLimits.MAX_EVENT_TYPE_LENGTH) {
            throw ProtocolCodecException("invalid_event_type")
        }
        listOf(value.correlationId, value.operationId, value.requestId, value.commandId)
            .filterNotNull()
            .forEach { requireProtocolId(it, "invalid_event_correlation_id") }
        value.commandType?.let {
            if (it.isBlank() || it.length > ProtocolLimits.MAX_COMMAND_TYPE_LENGTH) {
                throw ProtocolCodecException("invalid_event_command_type")
            }
        }
    }

    private fun encodeVersion(version: ProtocolVersion): JSONObject = JSONObject()
        .put("major", version.major)
        .put("minor", version.minor)

    private fun decodeVersion(root: JSONObject): ProtocolVersion = ProtocolVersion(
        major = root.optInt("major", -1),
        minor = root.optInt("minor", -1),
    ).also {
        if (it.major < 0 || it.minor < 0) throw ProtocolCodecException("invalid_protocol_version")
    }

    private fun encodeScope(scope: CommandScope): JSONObject = JSONObject().apply {
        scope.conversationId?.let { put("conversation_id", it) }
        scope.branchId?.let { put("branch_id", it) }
        scope.turnId?.let { put("turn_id", it) }
        scope.modelId?.let { put("model_id", it) }
        scope.documentId?.let { put("document_id", it) }
    }

    private fun parseRoot(bytes: ByteArray, code: String): JSONObject = try {
        JSONObject(String(bytes, StandardCharsets.UTF_8))
    } catch (_: Throwable) {
        throw ProtocolCodecException(code)
    }

    private fun parseJsonValue(value: String): Any = try {
        JSONTokener(value).nextValue() ?: JSONObject.NULL
    } catch (_: Throwable) {
        throw ProtocolCodecException("invalid_command_payload_json")
    }

    private fun jsonValueToString(value: Any?): String = when (value) {
        null, JSONObject.NULL -> "null"
        is JSONObject, is JSONArray -> value.toString()
        is String -> JSONObject.quote(value)
        is Boolean, is Number -> value.toString()
        else -> JSONObject.quote(value.toString())
    }

    private fun requiredObject(root: JSONObject, key: String): JSONObject =
        root.optJSONObject(key) ?: throw ProtocolCodecException("missing_$key")

    private fun requiredString(root: JSONObject, key: String): String =
        optionalString(root, key) ?: throw ProtocolCodecException("missing_$key")

    private fun optionalString(root: JSONObject, key: String): String? =
        if (!root.has(key) || root.isNull(key)) null else root.optString(key).takeIf { it.isNotBlank() }

    private fun parseInstant(value: String, code: String): Instant = try {
        Instant.parse(value)
    } catch (_: Throwable) {
        throw ProtocolCodecException(code)
    }

    private fun requireProtocolId(value: String, code: String) {
        val valid = value.isNotBlank() &&
            value.length <= ProtocolLimits.MAX_PROTOCOL_ID_LENGTH &&
            value == value.trim() &&
            value.all { character ->
                character.isLetterOrDigit() || character == '-' || character == '_' ||
                    character == '.' || character == ':' || character == '/'
            }
        if (!valid) throw ProtocolCodecException(code)
    }
}
