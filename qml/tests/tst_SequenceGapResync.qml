import QtQuick
import QtTest
import "../architecture"
import "../events"
import "../stores"

TestCase {
    id: testCase
    name: "SequenceGapResync"

    property var fakeAgent: null
    readonly property string streamId: "download:model:test-model"

    SignalSpy {
        id: eventSpy
        target: EventDispatcher
        signalName: "eventReceived"
    }

    Component {
        id: fakeAgentComponent
        QtObject {
            property int requestCount: 0
            signal async_result(string resultJson)

            function download_jobs_json(limit) {
                requestCount += 1
                return JSON.stringify({
                    accepted: true,
                    domain: "downloads.snapshot",
                    request_id: "download-resync-" + requestCount
                })
            }

            function completeLatest(payload) {
                async_result(JSON.stringify({
                    domain: "downloads.snapshot",
                    request_id: "download-resync-" + requestCount,
                    current: true,
                    ok: true,
                    payload: payload || []
                }))
            }

            function failLatest() {
                async_result(JSON.stringify({
                    domain: "downloads.snapshot",
                    request_id: "download-resync-" + requestCount,
                    current: true,
                    ok: false,
                    error: {
                        code: "ERR_TEST_SNAPSHOT",
                        severity: "warning",
                        recoverable: true,
                        safe_message: "Snapshot failed"
                    }
                }))
            }
        }
    }

    function init() {
        EventDispatcher.reset()
        SnapshotController.reset()
        eventSpy.clear()
        fakeAgent = fakeAgentComponent.createObject(testCase)
        verify(fakeAgent !== null)
        DownloadStore.configure(fakeAgent)
        DownloadStore.jobs.clear()
        DownloadStore.loading = false
        DownloadStore.hydrated = false
        DownloadStore.lastRequestId = ""
        // Force singleton construction so its Connections own the resync flow.
        verify(AppCoordinator !== null)
    }

    function cleanup() {
        DownloadStore.configure(null)
        DownloadStore.jobs.clear()
        DownloadStore.loading = false
        DownloadStore.hydrated = false
        DownloadStore.lastRequestId = ""
        if (fakeAgent !== null)
            fakeAgent.destroy()
        fakeAgent = null
        EventDispatcher.reset()
        SnapshotController.reset()
        eventSpy.clear()
    }

    function downloadEvent(eventId, sequence, bytesDownloaded) {
        return {
            protocol_version: { major: 2, minor: 0 },
            event_id: eventId,
            stream_id: streamId,
            sequence: sequence,
            event_type: "download_progress",
            emitted_at: "2026-07-13T09:00:00.000Z",
            payload: {
                category: "download_progress",
                state: "downloading",
                progress: bytesDownloaded / 100,
                bytes_downloaded: bytesDownloaded,
                total_bytes: 100,
                model_id: "test-model",
                destination: "model:test-model"
            }
        }
    }

    function test_snapshot_in_flight_keeps_quarantine_and_retries_stale_watermark() {
        EventDispatcher.ingest(JSON.stringify(downloadEvent("event-1", 1, 10)))
        compare(eventSpy.count, 1)

        EventDispatcher.ingest(JSON.stringify(downloadEvent("event-3", 3, 30)))
        compare(eventSpy.count, 1)
        verify(EventDispatcher.uncertainStreams[streamId] === true)
        compare(fakeAgent.requestCount, 1)
        verify(DownloadStore.loading)

        var firstTicket = SnapshotController.ticketForStream(streamId)
        verify(firstTicket !== null)
        compare(firstTicket.state, "snapshot_in_flight")
        compare(firstTicket.requestWatermark, 3)

        // This event arrives while the first snapshot is in flight. It must stay
        // quarantined and raise the required resync watermark to 4.
        EventDispatcher.ingest(JSON.stringify(downloadEvent("event-4", 4, 40)))
        compare(eventSpy.count, 1)
        var advancedTicket = SnapshotController.ticketForStream(streamId)
        compare(advancedTicket.requiredWatermark, 4)

        // The first snapshot only covers the watermark captured when it started
        // (3), so its successful application is stale relative to quarantine (4).
        fakeAgent.completeLatest([])
        verify(EventDispatcher.uncertainStreams[streamId] === true)
        verify(SnapshotController.isPending("downloads"))

        // Stale completion schedules a fresh snapshot at the advanced watermark.
        tryCompare(fakeAgent, "requestCount", 2)
        verify(DownloadStore.loading)
        var retryTicket = SnapshotController.ticketForStream(streamId)
        compare(retryTicket.state, "snapshot_in_flight")
        compare(retryTicket.requestWatermark, 4)

        fakeAgent.completeLatest([])
        tryVerify(function() {
            return EventDispatcher.uncertainStreams[streamId] !== true
        })
        verify(!SnapshotController.isPending("downloads"))

        // Sequence 5 is the first event accepted after the snapshot that covered
        // every sequence observed during quarantine.
        EventDispatcher.ingest(JSON.stringify(downloadEvent("event-5", 5, 50)))
        compare(eventSpy.count, 2)
        compare(EventDispatcher.lastSequenceByStream[streamId], 5)
    }

    function test_failed_snapshot_never_clears_quarantine() {
        EventDispatcher.ingest(JSON.stringify(downloadEvent("failure-1", 1, 10)))
        EventDispatcher.ingest(JSON.stringify(downloadEvent("failure-3", 3, 30)))
        verify(EventDispatcher.uncertainStreams[streamId] === true)
        compare(fakeAgent.requestCount, 1)

        fakeAgent.failLatest()
        verify(EventDispatcher.uncertainStreams[streamId] === true)
        var ticket = SnapshotController.ticketForStream(streamId)
        verify(ticket !== null)
        compare(ticket.state, "snapshot_failed")

        EventDispatcher.ingest(JSON.stringify(downloadEvent("failure-4", 4, 40)))
        compare(eventSpy.count, 1)
        verify(EventDispatcher.uncertainStreams[streamId] === true)
    }
}
