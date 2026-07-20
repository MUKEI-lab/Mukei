package ai.mukei.android

import ai.mukei.android.protocol.ProtocolVersion
import ai.mukei.android.protocol.TemporaryChatEndV2
import ai.mukei.android.protocol.TemporaryChatSessionV2
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class BackendRuntimePoliciesTest {
    @Test
    fun rejectedSubmissionRemovesOnlyOptimisticMessageAndClearsBusyState() {
        val existing = BackendRuntimeHost.ChatMessage(
            id = "existing",
            role = BackendRuntimeHost.ChatRole.USER,
            text = "old",
        )
        val optimistic = BackendRuntimeHost.ChatMessage(
            id = "optimistic",
            role = BackendRuntimeHost.ChatRole.USER,
            text = "new",
        )
        val state = BackendRuntimeHost.ConversationState(
            conversationId = "conversation",
            branchId = "branch",
            temporary = false,
            ragEnabled = true,
            messages = listOf(existing, optimistic),
            activeOperationId = "operation",
        )

        val rolledBack = BackendRuntimePolicies.rollbackRejectedSubmission(
            state = state,
            optimisticMessageId = optimistic.id,
            errorCode = "backend_unavailable",
        )

        assertEquals(listOf(existing), rolledBack.messages)
        assertNull(rolledBack.activeOperationId)
        assertEquals("backend_unavailable", rolledBack.lastErrorCode)
        assertFalse(rolledBack.busy)
    }

    @Test
    fun mismatchedTemporarySessionRequiresPurgeAndCleanupMustReportEnded() {
        val session = TemporaryChatSessionV2(
            protocolVersion = ProtocolVersion.CURRENT,
            runtimeSessionId = "actual-runtime",
            conversationId = "temporary-conversation",
            branchId = "temporary-branch",
            ragEnabled = false,
        )
        assertFalse(
            BackendRuntimePolicies.temporaryStartMatches(
                session = session,
                expectedRuntimeSessionId = "expected-runtime",
            ),
        )

        val notEnded = TemporaryChatEndV2(
            protocolVersion = ProtocolVersion.CURRENT,
            runtimeSessionId = session.runtimeSessionId,
            conversationId = session.conversationId,
            branchId = session.branchId,
            ended = false,
            ragEnabled = false,
        )
        assertFalse(BackendRuntimePolicies.temporaryCleanupMatches(notEnded, session))

        val ended = notEnded.copy(ended = true)
        assertTrue(BackendRuntimePolicies.temporaryCleanupMatches(ended, session))
    }

    @Test
    fun normalTemporaryExitRequiresPositiveEndedReceipt() {
        val receipt = TemporaryChatEndV2(
            protocolVersion = ProtocolVersion.CURRENT,
            runtimeSessionId = "runtime",
            conversationId = "conversation",
            branchId = "branch",
            ended = false,
            ragEnabled = false,
        )

        assertFalse(
            BackendRuntimePolicies.temporaryEndMatches(
                ended = receipt,
                expectedRuntimeSessionId = "runtime",
                conversationId = "conversation",
                branchId = "branch",
            ),
        )
        assertTrue(
            BackendRuntimePolicies.temporaryEndMatches(
                ended = receipt.copy(ended = true),
                expectedRuntimeSessionId = "runtime",
                conversationId = "conversation",
                branchId = "branch",
            ),
        )
    }
}
