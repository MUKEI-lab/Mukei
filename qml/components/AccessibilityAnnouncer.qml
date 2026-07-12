import QtQuick
import "../theme"

Item {
    id: root
    property string pendingText: ""
    property string lastAnnouncement: ""
    property int batchIntervalMs: Theme.reduceMotion ? 500 : 900
    property int maximumAnnouncementCharacters: 700

    signal announcementReady(string text)

    Timer {
        id: batchTimer
        interval: root.batchIntervalMs
        repeat: false
        onTriggered: root.flush()
    }

    function enqueueChunk(chunk) {
        var value = typeof chunk === "string" ? chunk : ""
        if (value.length === 0)
            return
        pendingText += value
        if (pendingText.length >= maximumAnnouncementCharacters)
            flush()
        else
            batchTimer.restart()
    }

    function announceStatus(text) {
        flush()
        var value = typeof text === "string" ? text.trim() : ""
        if (value.length > 0) {
            lastAnnouncement = value
            announcementReady(value)
        }
    }

    function flush() {
        batchTimer.stop()
        var value = pendingText.trim()
        pendingText = ""
        if (value.length === 0)
            return
        if (value.length > maximumAnnouncementCharacters)
            value = value.slice(0, maximumAnnouncementCharacters) + qsTr("…")
        lastAnnouncement = value
        announcementReady(value)
    }

    function reset() {
        batchTimer.stop()
        pendingText = ""
        lastAnnouncement = ""
    }
}
