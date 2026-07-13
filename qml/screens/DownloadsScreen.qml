import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import "../architecture"
import "../stores"
import "../theme"
import "../components"

Page {
    id: root
    background: Rectangle { color: Theme.p.background }
    Accessible.role: Accessible.Pane
    Accessible.name: qsTr("Model downloads")

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
            Text {
                Layout.fillWidth: true
                text: qsTr("Downloads")
                color: Theme.p.inkPrimary
                Component.onCompleted: Type.apply(this, Type.h1)
            }
            GhostButton {
                text: qsTr("Refresh")
                onClicked: IntentDispatcher.dispatch({ type: "downloads.refresh" })
            }
        }

        StoragePressureCard { Layout.fillWidth: true }

        ListView {
            id: list
            Layout.fillWidth: true
            Layout.fillHeight: true
            model: DownloadStore.jobs
            spacing: Spacing.sm
            clip: true
            reuseItems: true

            delegate: Rectangle {
                required property string jobId
                required property string modelId
                required property real progress
                required property string state
                required property real expectedBytes
                required property real bytesDownloaded
                required property string lastErrorCode
                width: ListView.view.width
                implicitHeight: content.implicitHeight + Spacing.lg * 2
                radius: Theme.radiusLg
                color: Theme.p.surface
                border.width: 1
                border.color: state === "failed" ? Theme.error : Theme.p.divider

                ColumnLayout {
                    id: content
                    anchors.fill: parent
                    anchors.margins: Spacing.lg
                    spacing: Spacing.sm
                    RowLayout {
                        Layout.fillWidth: true
                        Text {
                            Layout.fillWidth: true
                            text: modelId || qsTr("Model download")
                            color: Theme.p.inkPrimary
                            elide: Text.ElideRight
                            Component.onCompleted: Type.apply(this, Type.h3)
                        }
                        StatusPill {
                            text: state
                            subtype: state === "completed" ? "Success" : state === "failed" ? "Error" : "Neutral"
                        }
                    }
                    ProgressBar {
                        Layout.fillWidth: true
                        visible: ["queued", "starting", "downloading", "cancelling"].indexOf(state) >= 0
                        value: progress
                    }
                    Text {
                        text: expectedBytes > 0
                              ? qsTr("%1 of %2").arg(StorageStore.formatBytes(bytesDownloaded)).arg(StorageStore.formatBytes(expectedBytes))
                              : qsTr("Preparing download")
                        color: Theme.p.inkSecondary
                        Component.onCompleted: Type.apply(this, Type.bodySmall)
                    }
                    Text {
                        visible: lastErrorCode.length > 0
                        text: lastErrorCode
                        color: Theme.error
                        Component.onCompleted: Type.apply(this, Type.caption)
                    }
                    SecondaryButton {
                        visible: ["queued", "starting", "downloading"].indexOf(state) >= 0
                        enabled: CapabilityStore.canStopDownload
                        text: qsTr("Stop")
                        onClicked: IntentDispatcher.dispatch({ type: "download.cancel", jobId: jobId })
                    }
                }
            }

            footer: Item { width: 1; height: Spacing.xl }
        }

        Text {
            Layout.alignment: Qt.AlignHCenter
            visible: DownloadStore.count === 0
            text: qsTr("No model downloads yet.")
            color: Theme.p.inkSecondary
            Component.onCompleted: Type.apply(this, Type.bodyUI)
        }
    }

    Component.onCompleted: IntentDispatcher.dispatch({ type: "downloads.refresh" })
}
