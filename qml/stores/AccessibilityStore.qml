pragma Singleton
import QtQuick
import "../components"

Item {
    id: root
    readonly property alias pendingText: announcer.pendingText
    readonly property alias lastAnnouncement: announcer.lastAnnouncement
    property alias batchIntervalMs: announcer.batchIntervalMs
    property alias maximumAnnouncementCharacters: announcer.maximumAnnouncementCharacters

    signal announcementReady(string text)

    AccessibilityAnnouncer {
        id: announcer
        onAnnouncementReady: function(text) {
            root.announcementReady(text)
            if (typeof mukeiAccessibility !== "undefined" // qmllint disable unqualified
                    && mukeiAccessibility !== null // qmllint disable unqualified
                    && typeof mukeiAccessibility.announce === "function") // qmllint disable unqualified
                mukeiAccessibility.announce(text, false) // qmllint disable unqualified
        }
    }

    function enqueueChunk(chunk) { announcer.enqueueChunk(chunk) }
    function announceStatus(text) { announcer.announceStatus(text) }
    function flush() { announcer.flush() }
    function reset() { announcer.reset() }
}
