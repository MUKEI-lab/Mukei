pragma Singleton
import QtQuick

QtObject {
    id: root

    property var pendingStreams: ({})
    property int nextResyncGeneration: 0

    signal snapshotRequested(string feature, string streamId, string resyncId,
                             double expectedSequence, double snapshotWatermark)
    signal snapshotValidationFailed(string streamId, string resyncId, string reason)

    function reset() {
        pendingStreams = ({})
        nextResyncGeneration = 0
    }

    function ticketForStream(streamId) {
        if (!streamId || typeof pendingStreams[streamId] === "undefined")
            return null
        return Object.assign({}, pendingStreams[streamId])
    }

    function requestFeatureSnapshot(feature, streamId, expectedSequence, receivedSequence) {
        if (!feature || !streamId)
            return ""

        var observed = Number(receivedSequence)
        if (!Number.isFinite(observed) || Math.floor(observed) !== observed || observed < 1)
            return ""

        var existing = pendingStreams[streamId]
        if (existing) {
            noteObservedSequence(streamId, observed)
            return existing.resyncId || ""
        }

        nextResyncGeneration += 1
        var resyncId = "resync-" + nextResyncGeneration + "-" + Date.now()
        var ticket = {
            feature: feature,
            streamId: streamId,
            resyncId: resyncId,
            expectedSequence: Number(expectedSequence),
            requiredWatermark: observed,
            requestWatermark: observed,
            state: "snapshot_pending",
            attempt: 1,
            requestedAt: Date.now(),
            failureReason: ""
        }
        var next = Object.assign({}, pendingStreams)
        next[streamId] = ticket
        pendingStreams = next
        snapshotRequested(feature, streamId, resyncId, ticket.expectedSequence, observed)
        return resyncId
    }

    function noteObservedSequence(streamId, observedSequence) {
        var current = pendingStreams[streamId]
        if (!current)
            return false
        var observed = Number(observedSequence)
        if (!Number.isFinite(observed) || Math.floor(observed) !== observed || observed < 1)
            return false
        var ticket = Object.assign({}, current)
        ticket.requiredWatermark = Math.max(Number(ticket.requiredWatermark || 0), observed)
        var next = Object.assign({}, pendingStreams)
        next[streamId] = ticket
        pendingStreams = next
        return true
    }

    function markRequestStarted(streamId, resyncId, snapshotWatermark) {
        var current = pendingStreams[streamId]
        if (!current || current.resyncId !== resyncId)
            return false
        var watermark = Number(snapshotWatermark)
        if (!Number.isFinite(watermark) || Math.floor(watermark) !== watermark || watermark < 1)
            return false
        var ticket = Object.assign({}, current)
        ticket.requestWatermark = watermark
        ticket.state = "snapshot_in_flight"
        ticket.failureReason = ""
        var next = Object.assign({}, pendingStreams)
        next[streamId] = ticket
        pendingStreams = next
        return true
    }

    function markWaiting(streamId, resyncId) {
        var current = pendingStreams[streamId]
        if (!current || current.resyncId !== resyncId)
            return false
        var ticket = Object.assign({}, current)
        ticket.state = "waiting_for_snapshot_slot"
        var next = Object.assign({}, pendingStreams)
        next[streamId] = ticket
        pendingStreams = next
        return true
    }

    function markFailed(streamId, resyncId, reason) {
        var current = pendingStreams[streamId]
        if (!current || current.resyncId !== resyncId)
            return false
        var ticket = Object.assign({}, current)
        ticket.state = "snapshot_failed"
        ticket.failureReason = reason || "snapshot_failed"
        var next = Object.assign({}, pendingStreams)
        next[streamId] = ticket
        pendingStreams = next
        snapshotValidationFailed(streamId, resyncId, ticket.failureReason)
        return true
    }

    function validateApplied(streamId, resyncId, snapshotWatermark) {
        var current = pendingStreams[streamId]
        if (!current || current.resyncId !== resyncId || current.state !== "snapshot_in_flight")
            return false

        var watermark = Number(snapshotWatermark)
        if (!Number.isFinite(watermark) || Math.floor(watermark) !== watermark || watermark < 1) {
            markFailed(streamId, resyncId, "invalid_snapshot_watermark")
            return false
        }
        if (watermark < Number(current.requiredWatermark || 0)) {
            var stale = Object.assign({}, current)
            stale.state = "stale_snapshot"
            stale.failureReason = "snapshot_watermark_behind_quarantine"
            var next = Object.assign({}, pendingStreams)
            next[streamId] = stale
            pendingStreams = next
            snapshotValidationFailed(streamId, resyncId, stale.failureReason)
            return false
        }

        var ticket = Object.assign({}, current)
        ticket.state = "snapshot_validated"
        ticket.appliedWatermark = watermark
        ticket.failureReason = ""
        var updated = Object.assign({}, pendingStreams)
        updated[streamId] = ticket
        pendingStreams = updated
        return true
    }

    function markApplied(streamId, resyncId) {
        if (!streamId || !resyncId)
            return false
        var current = pendingStreams[streamId]
        if (!current || current.resyncId !== resyncId || current.state !== "snapshot_validated")
            return false
        var next = Object.assign({}, pendingStreams)
        delete next[streamId]
        pendingStreams = next
        return true
    }

    function retry(streamId, resyncId) {
        var current = pendingStreams[streamId]
        if (!current || current.resyncId !== resyncId)
            return false
        if (["stale_snapshot", "waiting_for_snapshot_slot"].indexOf(current.state) < 0)
            return false

        var ticket = Object.assign({}, current)
        ticket.state = "snapshot_pending"
        ticket.attempt = Number(ticket.attempt || 0) + 1
        ticket.requestWatermark = Number(ticket.requiredWatermark || 0)
        ticket.failureReason = ""
        var next = Object.assign({}, pendingStreams)
        next[streamId] = ticket
        pendingStreams = next
        snapshotRequested(ticket.feature, streamId, resyncId,
                          Number(ticket.expectedSequence || 0), ticket.requestWatermark)
        return true
    }

    function ticketsForFeature(feature, states) {
        var wanted = Array.isArray(states) ? states : []
        var result = []
        var streamIds = Object.keys(pendingStreams)
        for (var i = 0; i < streamIds.length; ++i) {
            var ticket = pendingStreams[streamIds[i]]
            if (ticket.feature !== feature)
                continue
            if (wanted.length > 0 && wanted.indexOf(ticket.state) < 0)
                continue
            result.push(Object.assign({}, ticket))
        }
        return result
    }

    function inFlightTicketsForFeature(feature) {
        return ticketsForFeature(feature, ["snapshot_in_flight"])
    }

    function waitingTicketsForFeature(feature) {
        return ticketsForFeature(feature, ["waiting_for_snapshot_slot"])
    }

    function isPending(feature) {
        return ticketsForFeature(feature, []).length > 0
    }
}
