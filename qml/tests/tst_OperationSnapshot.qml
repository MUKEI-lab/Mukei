import QtQuick
import QtTest
import "../stores"

TestCase {
    name: "OperationSnapshot"

    QtObject {
        id: agent
        function operation_snapshot_json() {
            return JSON.stringify({
                schema_version: 1,
                operations: [
                    {
                        operation_id: "download:1",
                        type: "download",
                        state: "downloading",
                        progress: 0.5,
                        cancelable: true,
                        retryable: false,
                        label: "Downloading model"
                    },
                    {
                        operation_id: "document_ingestion:1",
                        type: "document_ingestion",
                        state: "blocked",
                        phase: "waiting_for_embedder",
                        progress: 0,
                        cancelable: false,
                        retryable: true,
                        label: "Waiting for document embedder"
                    }
                ]
            })
        }
    }

    function init() {
        OperationStore.configure(agent)
        OperationStore.hydrate()
    }

    function test_active_and_blocked_counts_are_distinct() {
        compare(OperationStore.totalCount, 2)
        compare(OperationStore.activeCount, 1)
        compare(OperationStore.blockedCount, 1)
        verify(OperationStore.hasActiveOperations)
    }

    function test_snapshot_roles_are_normalised() {
        var row = OperationStore.operations.get(OperationStore.findById("document_ingestion:1"))
        compare(row.phase, "waiting_for_embedder")
        verify(row.retryable)
        compare(row.progress, 0)
    }
}
