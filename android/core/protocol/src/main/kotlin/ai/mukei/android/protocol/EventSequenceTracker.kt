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
 * Process-local guard against duplicate/out-of-order event projection.
 *
 * A new runtime session resets sequence history. Forward gaps are reported so the
 * repository layer can request an authoritative snapshot/reconciliation instead of
 * silently pretending the event stream is complete.
 */
class EventSequenceTracker {
    private var runtimeSessionId: String? = null
    private val lastSequenceByStream = mutableMapOf<String, Long>()

    @Synchronized
    fun accept(batch: EventBatchV2): EventSequenceValidation {
        val previousSession = runtimeSessionId
        val sessionChanged = previousSession != null && previousSession != batch.runtimeSessionId
        if (previousSession == null || sessionChanged) {
            runtimeSessionId = batch.runtimeSessionId
            lastSequenceByStream.clear()
        }

        val gaps = mutableListOf<EventSequenceGap>()
        for (event in batch.events) {
            val previous = lastSequenceByStream[event.streamId]
            if (previous != null) {
                if (event.sequence <= previous) {
                    throw ProtocolCodecException("event_sequence_not_monotonic")
                }
                val expected = previous + 1L
                if (event.sequence > expected) {
                    gaps += EventSequenceGap(
                        streamId = event.streamId,
                        expectedSequence = expected,
                        actualSequence = event.sequence,
                    )
                }
            }
            lastSequenceByStream[event.streamId] = event.sequence
        }

        return EventSequenceValidation(
            runtimeSessionChanged = sessionChanged,
            gaps = gaps,
        )
    }

    @Synchronized
    fun reset() {
        runtimeSessionId = null
        lastSequenceByStream.clear()
    }
}
