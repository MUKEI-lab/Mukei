import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Dialogs
import QtQuick.Layouts
import "../architecture"
import "../stores"
import "../theme"
import "../components"

Page {
    id: root
    property string pendingRevokeDocumentId: ""
    property string pendingRevokeDocumentName: ""
    background: Rectangle { color: Theme.p.background }
    Accessible.role: Accessible.Pane
    Accessible.name: qsTr("Private documents")

    function filenameFromUrl(url) {
        var text = String(url || "")
        var slash = Math.max(text.lastIndexOf("/"), text.lastIndexOf("\\"))
        var value = slash >= 0 ? text.slice(slash + 1) : text
        try { return decodeURIComponent(value) } catch (error) { return value }
    }

    FileDialog {
        id: documentPicker
        title: qsTr("Choose a private document")
        fileMode: FileDialog.OpenFile
        nameFilters: [
            qsTr("Documents (*.txt *.md *.pdf *.json *.csv)"),
            qsTr("All files (*)")
        ]
        onAccepted: {
            var target = selectedFile.toString()
            IntentDispatcher.dispatch({
                type: "documents.grant",
                target: target,
                label: root.filenameFromUrl(target),
                mimeType: "application/octet-stream"
            })
        }
    }

    DestructiveConfirmDialog {
        id: revokeDialog
        heading: qsTr("Revoke document access?")
        body: qsTr("Mukei will remove access to %1 and schedule deletion of its local chunks and vectors. This action cannot be undone.").arg(root.pendingRevokeDocumentName)
        destructiveText: qsTr("Revoke access")
        onDestructiveCommitted: {
            close()
            IntentDispatcher.dispatch({
                type: "documents.revoke",
                documentId: root.pendingRevokeDocumentId
            })
        }
    }

    ColumnLayout {
        anchors.fill: parent
        anchors.margins: ResponsiveStore.compact ? Spacing.md : Spacing.xl
        spacing: Spacing.lg
        RowLayout {
            Layout.fillWidth: true
            IconButton {
                visible: ResponsiveStore.compact
                iconSource: "qrc:/icons/back.svg"
                text: qsTr("Back")
                onClicked: IntentDispatcher.dispatch({ type: "navigation.back" })
            }
            ColumnLayout {
                Layout.fillWidth: true
                spacing: Spacing.xxs
                Text {
                    Layout.fillWidth: true
                    text: qsTr("Private documents")
                    color: Theme.p.inkPrimary
                    Component.onCompleted: Type.apply(this, Type.h1)
                }
                Text {
                    Layout.fillWidth: true
                    text: qsTr("Document access is tracked and revocable. Indexing begins only after native permission and ingestion checks succeed.")
                    color: Theme.p.inkSecondary
                    wrapMode: Text.Wrap
                    Component.onCompleted: Type.apply(this, Type.bodySmall)
                }
            }
            SecondaryButton {
                text: qsTr("Add document")
                onClicked: documentPicker.open()
            }
            GhostButton {
                text: qsTr("Refresh")
                onClicked: IntentDispatcher.dispatch({ type: "documents.refresh" })
            }
        }

        Rectangle {
            Layout.fillWidth: true
            visible: DocumentStore.cleanupPendingCount > 0
            implicitHeight: pendingText.implicitHeight + Spacing.lg * 2
            radius: Theme.radiusLg
            color: Qt.rgba(Theme.warning.r, Theme.warning.g, Theme.warning.b, 0.12)
            border.width: 1
            border.color: Theme.warning
            Text {
                id: pendingText
                anchors.fill: parent
                anchors.margins: Spacing.lg
                text: qsTr("%1 private document cleanup operation(s) will retry automatically.").arg(DocumentStore.cleanupPendingCount)
                color: Theme.p.inkPrimary
                wrapMode: Text.Wrap
                Component.onCompleted: Type.apply(this, Type.bodyUI)
            }
        }

        ListView {
            Layout.fillWidth: true
            Layout.fillHeight: true
            model: DocumentStore.documents
            spacing: Spacing.sm
            clip: true
            reuseItems: true
            cacheBuffer: Math.max(height, 600)
            delegate: Rectangle {
                required property string documentId
                required property string label
                required property string mimeType
                required property real sizeBytes
                required property real chunkCount
                required property bool revoked
                required property bool cleanupPending
                required property real cleanupAttempts
                required property string permissionState
                required property string ingestionState
                required property real ingestionProgress
                required property bool ingestionRetryable
                required property string ingestionError
                width: ListView.view.width
                implicitHeight: rowContent.implicitHeight + Spacing.lg * 2
                radius: Theme.radiusLg
                color: Theme.p.surface
                border.width: 1
                border.color: cleanupPending ? Theme.warning : Theme.p.divider
                RowLayout {
                    id: rowContent
                    anchors.fill: parent
                    anchors.margins: Spacing.lg
                    spacing: Spacing.md
                    MukeiIcon { name: "file"; size: 24; tone: revoked ? Theme.p.inkFaint : Theme.p.inkPrimary }
                    ColumnLayout {
                        Layout.fillWidth: true
                        Text {
                            Layout.fillWidth: true
                            text: label
                            color: Theme.p.inkPrimary
                            elide: Text.ElideMiddle
                            Component.onCompleted: Type.apply(this, Type.h3)
                        }
                        Text {
                            Layout.fillWidth: true
                            text: chunkCount > 0
                                  ? qsTr("%1 chunks · %2").arg(chunkCount).arg(StorageStore.formatBytes(sizeBytes))
                                  : ingestionState === "failed"
                                    ? qsTr("Indexing paused · retry available")
                                    : ingestionState === "completed"
                                      ? qsTr("Indexed · %1").arg(StorageStore.formatBytes(sizeBytes))
                                      : ingestionState === "waiting_for_embedder"
                                        ? qsTr("Private access retained · waiting for the on-device indexer")
                                      : permissionState === "persisted"
                                        ? qsTr("Private access retained · indexing %1").arg(ingestionState)
                                        : permissionState === "transient"
                                          ? qsTr("Temporary access · reselect may be required after restart")
                                          : qsTr("Access registered · indexing %1").arg(ingestionState)
                            color: Theme.p.inkSecondary
                            wrapMode: Text.Wrap
                            Component.onCompleted: Type.apply(this, Type.bodySmall)
                        }
                        ProgressBar {
                            Layout.fillWidth: true
                            visible: ["reading", "chunking", "embedding", "committing"].indexOf(ingestionState) >= 0
                            value: Math.max(0, Math.min(1, ingestionProgress / 100))
                        }
                    }
                    StatusPill {
                        text: cleanupPending ? qsTr("Cleanup pending")
                            : revoked ? qsTr("Revoked")
                            : ingestionState === "failed" ? qsTr("Indexing failed")
                            : chunkCount > 0 || ingestionState === "completed" ? qsTr("Indexed")
                            : ingestionState === "queued" ? qsTr("Queued")
                            : ingestionState === "waiting_for_embedder" ? qsTr("Waiting") : qsTr("Granted")
                        subtype: cleanupPending || ingestionState === "failed" ? "Warning"
                            : revoked ? "Neutral" : "Success"
                    }
                    GhostButton {
                        visible: !revoked && ingestionState === "failed" && ingestionRetryable
                        text: qsTr("Retry")
                        onClicked: IntentDispatcher.dispatch({
                            type: "documents.retryIngestion",
                            documentId: documentId
                        })
                    }
                    DestructiveButton {
                        visible: !revoked
                        enabled: !cleanupPending
                        text: qsTr("Revoke")
                        onCommitted: {
                            root.pendingRevokeDocumentId = documentId
                            root.pendingRevokeDocumentName = label
                            revokeDialog.open()
                        }
                    }
                }
            }
        }

        Text {
            Layout.alignment: Qt.AlignHCenter
            visible: DocumentStore.count === 0
            text: qsTr("No private documents are registered yet.")
            color: Theme.p.inkSecondary
            Component.onCompleted: Type.apply(this, Type.bodyUI)
        }
    }

    Component.onCompleted: IntentDispatcher.dispatch({ type: "documents.refresh" })
}
