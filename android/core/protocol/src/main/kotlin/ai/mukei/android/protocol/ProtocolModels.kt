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

enum class ClientKind {
    ANDROID,
}

data class ProtocolCapabilitySnapshot(
    val currentVersion: ProtocolVersion,
    val minimumSupportedPeerMajor: Int,
    val capabilities: List<String>,
)

data class RuntimeContractSnapshot(
    val clientKind: ClientKind,
    val runtimeSessionId: String,
    val protocol: ProtocolCapabilitySnapshot,
    val snapshotSchemaVersion: Int,
)

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

enum class AcknowledgementStatus {
    ACCEPTED,
    REJECTED,
}

data class CommandAcknowledgementV2(
    val protocolVersion: ProtocolVersion,
    val commandId: String,
    val requestId: String,
    val correlationId: String,
    val operationId: String? = null,
    val status: AcknowledgementStatus,
    val rejectionReason: String? = null,
    val timestamp: Instant,
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
    val requestId: String? = null,
    val commandId: String? = null,
    val commandType: String? = null,
    val payloadJson: String,
)

data class EventBatchV2(
    val protocolVersion: ProtocolVersion,
    val runtimeSessionId: String,
    val drainedAt: Instant,
    val events: List<EventEnvelopeV2>,
    val hasMore: Boolean,
)

enum class SnapshotDomainV2 {
    APPLICATION,
    SETTINGS,
    PROTOCOL,
    OPERATIONS,
}

data class SnapshotEnvelopeV2(
    val protocolVersion: ProtocolVersion,
    val runtimeSessionId: String,
    val domain: SnapshotDomainV2,
    val schemaVersion: Int,
    val generatedAt: Instant,
    val payloadJson: String,
)

object ProtocolLimits {
    const val MAX_COMMAND_ENVELOPE_BYTES: Int = 64 * 1024
    const val MAX_EVENT_BATCH_BYTES: Int = 512 * 1024
    const val MAX_EVENT_BATCH_ITEMS: Int = 256
    const val MAX_PROTOCOL_ID_LENGTH: Int = 128
    const val MAX_COMMAND_TYPE_LENGTH: Int = 96
    const val MAX_EVENT_TYPE_LENGTH: Int = 128
    const val MAX_IDEMPOTENCY_KEY_LENGTH: Int = 192
}
