pragma Singleton
import QtQuick

QtObject {
    property var pendingFeatures: ({})
    signal snapshotRequested(string feature, int expectedSequence, int receivedSequence)

    function requestFeatureSnapshot(feature, expectedSequence, receivedSequence) {
        var next = Object.assign({}, pendingFeatures)
        next[feature] = {
            expectedSequence: expectedSequence,
            receivedSequence: receivedSequence,
            requestedAt: Date.now()
        }
        pendingFeatures = next
        snapshotRequested(feature, expectedSequence, receivedSequence)
    }

    function markApplied(feature) {
        if (typeof pendingFeatures[feature] === "undefined")
            return
        var next = Object.assign({}, pendingFeatures)
        delete next[feature]
        pendingFeatures = next
    }

    function isPending(feature) {
        return typeof pendingFeatures[feature] !== "undefined"
    }
}
