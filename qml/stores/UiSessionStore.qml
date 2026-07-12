pragma Singleton
import QtQuick

Item {
    id: root

    property int schemaVersion: 1
    property string profileId: "default"
    property string activeRoute: "boot"
    property var activeRouteParameters: ({})
    property string activeConversationId: ""
    property string activeBranchId: ""
    property string timelineAnchorMessageId: ""
    property string selectedModelId: ""
    property var drafts: ({})
    property var cursorPositions: ({})
    property var pendingDrafts: ({})
    property var agentSource: null
    property bool hydrated: false
    property bool loading: false
    property bool backendPersistenceAvailable: false
    property string sessionRequestId: ""
    property string draftRequestId: ""

    signal hydrationCompleted
    signal persistenceFailed(string feature)
    signal draftLoaded(string conversationId, string branchId, string text, int cursorPosition)

    Timer {
        id: sessionFlush
        interval: 300
        repeat: false
        onTriggered: root.flushSession()
    }

    Timer {
        id: draftFlush
        interval: 400
        repeat: false
        onTriggered: root.flushDrafts()
    }

    function configure(agent) {
        agentSource = agent
        backendPersistenceAvailable = agentSource !== null
                && typeof agentSource.ui_session_json === "function"
                && typeof agentSource.save_ui_session === "function"
    }

    function safeParse(jsonText, fallback) {
        if (typeof jsonText !== "string" || jsonText.length === 0)
            return fallback
        try {
            var value = JSON.parse(jsonText)
            return value === null ? fallback : value
        } catch (error) {
            return fallback
        }
    }

    function cacheDraftRecord(record) {
        if (!record)
            return
        var conversationId = record.conversation_id || ""
        var branchId = record.branch_id || ""
        if (!conversationId || !branchId)
            return
        var key = draftKey(conversationId, branchId)
        var next = Object.assign({}, drafts)
        next[key] = record.text || ""
        drafts = next
        var cursors = Object.assign({}, cursorPositions)
        cursors[key] = Math.max(0, Number(record.cursor_position || 0))
        cursorPositions = cursors
        draftLoaded(conversationId, branchId, next[key], cursors[key])
    }

    function applySessionPayload(payload) {
        var value = payload && payload.session !== undefined ? payload.session : payload
        if (value && !value.error && value.schema_version === schemaVersion) {
            activeRoute = value.active_route || "boot"
            activeConversationId = value.active_conversation_id || ""
            activeBranchId = value.active_branch_id || ""
            timelineAnchorMessageId = value.timeline_anchor_message_id || ""
            selectedModelId = value.selected_model_id || ""
            var routePayload = safeParse(value.payload_json || "{}", ({}))
            activeRouteParameters = routePayload.route_parameters || ({})
        }
        if (payload && payload.active_draft)
            cacheDraftRecord(payload.active_draft)
        loading = false
        hydrated = true
        hydrationCompleted()
    }

    function hydrate() {
        if (loading)
            return
        hydrated = false
        if (!backendPersistenceAvailable) {
            applySessionPayload(null)
            return
        }
        loading = true
        var value = safeParse(agentSource.ui_session_json(), null)
        if (value && value.accepted === true) {
            sessionRequestId = value.request_id || ""
            return
        }
        if (value && value.error)
            persistenceFailed("session_load")
        applySessionPayload(value)
    }

    function scheduleSessionFlush() {
        if (backendPersistenceAvailable)
            sessionFlush.restart()
    }

    function setActiveRoute(route, parameters) {
        activeRoute = route || "boot"
        activeRouteParameters = parameters || ({})
        scheduleSessionFlush()
    }

    function setActiveChatScope(conversationId, branchId) {
        activeConversationId = conversationId || ""
        activeBranchId = branchId || ""
        scheduleSessionFlush()
    }

    function setTimelineAnchor(messageId) {
        timelineAnchorMessageId = messageId || ""
        scheduleSessionFlush()
    }

    function setSelectedModel(modelId) {
        selectedModelId = modelId || ""
        scheduleSessionFlush()
    }

    function flushSession() {
        if (!backendPersistenceAvailable)
            return
        try {
            agentSource.save_ui_session(JSON.stringify({
                profile_id: profileId,
                schema_version: schemaVersion,
                active_route: activeRoute,
                active_conversation_id: activeConversationId,
                active_branch_id: activeBranchId,
                timeline_anchor_message_id: timelineAnchorMessageId,
                selected_model_id: selectedModelId,
                payload: { route_parameters: activeRouteParameters }
            }))
        } catch (error) {
            persistenceFailed("session")
        }
    }

    function draftKey(conversationId, branchId) {
        return (conversationId || "default") + ":" + (branchId || "main")
    }

    function saveDraft(conversationId, branchId, text, cursorPosition) {
        var key = draftKey(conversationId, branchId)
        var next = Object.assign({}, drafts)
        next[key] = text || ""
        drafts = next
        var cursors = Object.assign({}, cursorPositions)
        cursors[key] = Math.max(0, cursorPosition || 0)
        cursorPositions = cursors
        var pending = Object.assign({}, pendingDrafts)
        pending[key] = {
            conversationId: conversationId || "",
            branchId: branchId || "",
            text: text || "",
            cursorPosition: Math.max(0, cursorPosition || 0)
        }
        pendingDrafts = pending
        if (backendPersistenceAvailable)
            draftFlush.restart()
    }

    function flushDrafts() {
        if (!backendPersistenceAvailable)
            return
        var pending = pendingDrafts
        pendingDrafts = ({})
        for (var key in pending) {
            var draft = pending[key]
            try {
                agentSource.save_draft(draft.conversationId, draft.branchId,
                                       draft.text, draft.cursorPosition)
            } catch (error) {
                persistenceFailed("draft")
            }
        }
    }

    function loadDraft(conversationId, branchId) {
        var key = draftKey(conversationId, branchId)
        if (typeof drafts[key] === "string")
            return drafts[key]
        if (!backendPersistenceAvailable || typeof agentSource.draft_json !== "function")
            return ""
        var value = safeParse(agentSource.draft_json(conversationId || "", branchId || ""), null)
        if (value && value.accepted === true) {
            draftRequestId = value.request_id || ""
            return ""
        }
        if (value && !value.error) {
            cacheDraftRecord(value)
            return typeof drafts[key] === "string" ? drafts[key] : ""
        }
        return ""
    }

    function cursorPosition(conversationId, branchId) {
        var key = draftKey(conversationId, branchId)
        return typeof cursorPositions[key] === "number" ? cursorPositions[key] : 0
    }

    function clearDraft(conversationId, branchId) {
        var key = draftKey(conversationId, branchId)
        var next = Object.assign({}, drafts)
        delete next[key]
        drafts = next
        var cursors = Object.assign({}, cursorPositions)
        delete cursors[key]
        cursorPositions = cursors
        var pending = Object.assign({}, pendingDrafts)
        delete pending[key]
        pendingDrafts = pending
        if (backendPersistenceAvailable && typeof agentSource.clear_draft === "function") {
            try {
                agentSource.clear_draft(conversationId || "", branchId || "")
            } catch (error) {
                persistenceFailed("draft_clear")
            }
        }
    }

    function flushNow() {
        sessionFlush.stop()
        draftFlush.stop()
        flushSession()
        flushDrafts()
    }

    Connections {
        target: root.agentSource
        ignoreUnknownSignals: true
        function onAsync_result(resultJson) {
            var result = root.safeParse(resultJson, null)
            if (!result || result.current === false)
                return
            if (result.domain === "ui_session.snapshot"
                    && result.request_id === root.sessionRequestId) {
                if (result.ok === true)
                    root.applySessionPayload(result.payload)
                else {
                    root.loading = false
                    root.hydrated = true
                    root.persistenceFailed("session_load")
                    root.hydrationCompleted()
                }
            } else if (result.domain === "ui_session.draft"
                       && result.request_id === root.draftRequestId) {
                if (result.ok === true && result.payload)
                    root.cacheDraftRecord(result.payload.draft)
                else if (result.ok !== true)
                    root.persistenceFailed("draft_load")
            }
        }
    }
}
