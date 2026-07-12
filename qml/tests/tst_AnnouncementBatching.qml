import QtQuick
import QtTest
import "../components"

TestCase {
    name: "AnnouncementBatching"
    width: 320
    height: 200
    when: windowShown

    Component {
        id: fixtureFactory
        Item {
            property alias announcer: announcer
            property alias spy: spy
            AccessibilityAnnouncer {
                id: announcer
                batchIntervalMs: 30
                maximumAnnouncementCharacters: 80
            }
            SignalSpy { id: spy; target: announcer; signalName: "announcementReady" }
        }
    }

    function test_chunks_are_batched_until_flush() {
        var fixture = createTemporaryObject(fixtureFactory, this)
        fixture.announcer.enqueueChunk("Hello ")
        fixture.announcer.enqueueChunk("world")
        compare(fixture.spy.count, 0)
        fixture.announcer.flush()
        compare(fixture.spy.count, 1)
        compare(fixture.spy.signalArguments[0][0], "Hello world")
    }

    function test_maximum_length_forces_bounded_announcement() {
        var fixture = createTemporaryObject(fixtureFactory, this)
        fixture.announcer.maximumAnnouncementCharacters = 8
        fixture.announcer.enqueueChunk("123456789")
        compare(fixture.spy.count, 1)
        verify(fixture.spy.signalArguments[0][0].length <= 9)
    }
}
