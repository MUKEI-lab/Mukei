package ai.mukei.android

import ai.mukei.android.protocol.TemporaryChatEndV2
import ai.mukei.android.protocol.TemporaryChatSessionV2

/** Pure state/contract rules kept separate from Android threading so they are regression-testable. */
internal object BackendRuntimePolicies {
    fun rollbackRejectedSubmission(
        state: BackendRuntimeHost.ConversationState,
        optimisticMessageId: String,
        errorCode: String,
    ): BackendRuntimeHost.ConversationState = state.copy(
        messages = state.messages.filterNot { it.id == optimisticMessageId },
        streamingAssistant = "",
        activeOperationId = null,
        lastErrorCode = errorCode,
    )

    fun temporaryStartMatches(
        session: TemporaryChatSessionV2,
        expectedRuntimeSessionId: String,
    ): Boolean = session.runtimeSessionId == expectedRuntimeSessionId && !session.ragEnabled

    fun temporaryCleanupMatches(
        ended: TemporaryChatEndV2,
        session: TemporaryChatSessionV2,
    ): Boolean = ended.ended &&
        ended.runtimeSessionId == session.runtimeSessionId &&
        ended.conversationId == session.conversationId &&
        ended.branchId == session.branchId &&
        !ended.ragEnabled

    fun temporaryEndMatches(
        ended: TemporaryChatEndV2,
        expectedRuntimeSessionId: String,
        conversationId: String,
        branchId: String,
    ): Boolean = ended.ended &&
        ended.runtimeSessionId == expectedRuntimeSessionId &&
        ended.conversationId == conversationId &&
        ended.branchId == branchId &&
        !ended.ragEnabled
}
