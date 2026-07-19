package ai.mukei.android

import ai.mukei.android.protocol.EventEnvelopeV2
import org.json.JSONObject

enum class ChatSessionKind {
    NORMAL,
    TEMPORARY,
}

data class ActiveChatScope(
    val conversationId: String,
    val branchId: String,
    val kind: ChatSessionKind,
) {
    val temporary: Boolean
        get() = kind == ChatSessionKind.TEMPORARY
}

enum class ChatMessageRole {
    USER,
    ASSISTANT,
}

data class ChatUiMessage(
    val id: String,
    val role: ChatMessageRole,
    val text: String,
    val streaming: Boolean = false,
)

data class ChatUiState(
    val session: ActiveChatScope? = null,
    val messages: List<ChatUiMessage> = emptyList(),
    val generationInProgress: Boolean = false,
    val transitionInProgress: Boolean = false,
    val activeCorrelationId: String? = null,
    val activeOperationId: String? = null,
    val errorCode: String? = null,
) {
    val temporary: Boolean
        get() = session?.temporary == true

    val canSwitchSession: Boolean
        get() = !generationInProgress && !transitionInProgress
}

/** Pure reducer for native chat events that belong to the currently active UI session. */
object ChatEventReducer {
    fun reduce(state: ChatUiState, events: List<EventEnvelopeV2>): ChatUiState =
        events.fold(state, ::reduceOne)

    private fun reduceOne(state: ChatUiState, event: EventEnvelopeV2): ChatUiState {
        val session = state.session ?: return state
        val conversationStream = "conversation:${session.conversationId}"
        val belongsToActiveTurn = event.correlationId != null &&
            event.correlationId == state.activeCorrelationId
        val belongsToActiveOperation = event.operationId != null &&
            event.operationId == state.activeOperationId
        val belongsToConversation = event.streamId == conversationStream
        if (!belongsToActiveTurn && !belongsToActiveOperation && !belongsToConversation) {
            return state
        }

        return when (event.eventType) {
            "chat.token.delta" -> {
                if (!belongsToActiveTurn && !belongsToActiveOperation) return state
                val token = payload(event).optString("text")
                if (token.isEmpty()) state else state.appendAssistantToken(token)
            }

            "chat.generation.completed" -> {
                if (!belongsToActiveTurn && !belongsToConversation) return state
                val content = payload(event).optString("content")
                state.finishAssistant(content)
            }

            "operation.failed" -> {
                if (!belongsToActiveTurn && !belongsToActiveOperation) return state
                state.copy(
                    generationInProgress = false,
                    activeCorrelationId = null,
                    activeOperationId = null,
                    errorCode = payload(event).optString("code").ifBlank { "generation_failed" },
                ).finishStreamingMarker()
            }

            "operation.cancelled" -> {
                if (!belongsToActiveTurn && !belongsToActiveOperation) return state
                state.copy(
                    generationInProgress = false,
                    activeCorrelationId = null,
                    activeOperationId = null,
                    errorCode = null,
                ).finishStreamingMarker()
            }

            else -> state
        }
    }

    private fun ChatUiState.appendAssistantToken(token: String): ChatUiState {
        val last = messages.lastOrNull()
        val updated = if (last?.role == ChatMessageRole.ASSISTANT && last.streaming) {
            messages.dropLast(1) + last.copy(text = last.text + token)
        } else {
            messages + ChatUiMessage(
                id = "assistant:${activeCorrelationId.orEmpty()}",
                role = ChatMessageRole.ASSISTANT,
                text = token,
                streaming = true,
            )
        }
        return copy(messages = updated)
    }

    private fun ChatUiState.finishAssistant(content: String): ChatUiState {
        val last = messages.lastOrNull()
        val updated = when {
            content.isBlank() && last?.role == ChatMessageRole.ASSISTANT ->
                messages.dropLast(1) + last.copy(streaming = false)
            content.isBlank() -> messages
            last?.role == ChatMessageRole.ASSISTANT && last.streaming ->
                messages.dropLast(1) + last.copy(text = content, streaming = false)
            else -> messages + ChatUiMessage(
                id = "assistant:${activeCorrelationId.orEmpty()}",
                role = ChatMessageRole.ASSISTANT,
                text = content,
                streaming = false,
            )
        }
        return copy(
            messages = updated,
            generationInProgress = false,
            activeCorrelationId = null,
            activeOperationId = null,
            errorCode = null,
        )
    }

    private fun ChatUiState.finishStreamingMarker(): ChatUiState {
        val last = messages.lastOrNull() ?: return this
        return if (last.role == ChatMessageRole.ASSISTANT && last.streaming) {
            copy(messages = messages.dropLast(1) + last.copy(streaming = false))
        } else {
            this
        }
    }

    private fun payload(event: EventEnvelopeV2): JSONObject = try {
        JSONObject(event.payloadJson)
    } catch (_: Throwable) {
        JSONObject()
    }
}
