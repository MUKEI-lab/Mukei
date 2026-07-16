package ai.mukei.android.protocol

import java.time.Instant

data class ProtocolVersion(
    val major: Int,
    val minor: Int,
) {
    fun isCompatible(): Boolean = major == CURRENT.major

    companion object {
        val CURRENT = ProtocolVersion(major = 2, minor = 0)
    }
}

data class CommandScope(
    val conversationId: String? = null,
    val branchId: String? = null,
    val turnId: String? = null,
    val modelId: String? = null,
    val documentId: String? = null,
)

data class CommandEnvelopeV2(
    val protocolVersion: ProtocolVersion = ProtocolVersion.CURRENT,
    val commandId: String,
    val requestId: String,
    val commandType: String,
    val submittedAt: Instant,
    val operationId: String? = null,
    val correlationId: String,
    val idempotencyKey: String? = null,
    val scope: CommandScope? = null,
    val payloadJson: String,
)

data class CommandAcknowledgementV2(
    val accepted: Boolean,
    val commandId: String,
    val requestId: String,
    val correlationId: String,
    val operationId: String? = null,
    val rejectionReason: String? = null,
)

data class EventEnvelopeV2(
    val protocolVersion: ProtocolVersion,
    val eventId: String,
    val streamId: String,
    val sequence: Long,
    val eventType: String,
    val emittedAt: Instant,
    val correlationId: String? = null,
    val operationId: String? = null,
    val payloadJson: String,
)

object ProtocolLimits {
    const val MAX_COMMAND_ENVELOPE_BYTES: Int = 64 * 1024
    const val MAX_PROTOCOL_ID_LENGTH: Int = 128
    const val MAX_COMMAND_TYPE_LENGTH: Int = 96
    const val MAX_IDEMPOTENCY_KEY_LENGTH: Int = 192
}
