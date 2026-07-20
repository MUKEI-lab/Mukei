package ai.mukei.android.protocol

/** One detected forward gap in an otherwise monotonic event stream. */
data class EventSequenceGap(
    val streamId: String,
    val expectedSequence: Long,
    val actualSequence: Long,
)

/** Result of accepting one decoded event batch. */
data class EventSequenceValidation(
    val runtimeSessionChanged: Boolean,
    val gaps: List<EventSequenceGap>,
)

/**
 * Process-local guard against duplicate, out-of-order, or incomplete event projection.
 *
 * Until the Android repository layer implements domain-specific snapshot replay, a forward
 * sequence gap or an unexpected runtime-session change must fail closed. Continuing after either
 * condition could project a partial operation lifecycle and leave UI state permanently stale.
 */
class EventSequenceTracker {
    private var runtimeSessionId: String? = null
    private val lastSequenceByStream = mutableMapOf<String, Long>()

    @Synchronized
    fun accept(batch: EventBatchV2): EventSequenceValidation {
        val previousSession = runtimeSessionId
        if (previousSession != null && previousSession != batch.runtimeSessionId) {
            throw ProtocolCodecException("runtime_session_changed")
        }
        if (previousSession == null) {
            runtimeSessionId = batch.runtimeSessionId
        }

        for (event in batch.events) {
            val previous = lastSequenceByStream[event.streamId]
            if (previous != null) {
                if (event.sequence <= previous) {
                    throw ProtocolCodecException("event_sequence_not_monotonic")
                }
                val expected = previous + 1L
                if (event.sequence > expected) {
                    throw ProtocolCodecException("event_sequence_gap")
                }
            }
            lastSequenceByStream[event.streamId] = event.sequence
        }

        return EventSequenceValidation(
            runtimeSessionChanged = false,
            gaps = emptyList(),
        )
    }

    @Synchronized
    fun reset() {
        runtimeSessionId = null
        lastSequenceByStream.clear()
    }
}
