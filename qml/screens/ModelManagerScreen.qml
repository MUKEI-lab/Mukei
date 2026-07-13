import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import "../architecture"
import "../stores"
import "../theme"
import "../components"

Page {
    id: root
    property string pendingDeleteModelId: ""
    property string pendingDeleteModelName: ""
    background: Rectangle { color: Theme.p.background }
    Accessible.role: Accessible.Pane
    Accessible.name: qsTr("Model manager")

    DestructiveConfirmDialog {
        id: deleteDialog
        heading: qsTr("Delete model?")
        body: qsTr("%1 will be removed from this device. Conversations remain safe, but the model must be downloaded again before it can be selected.").arg(root.pendingDeleteModelName)
        destructiveText: qsTr("Delete model")
        onDestructiveCommitted: {
            close()
            IntentDispatcher.dispatch({ type: "model.delete", modelId: root.pendingDeleteModelId })
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
                    text: qsTr("Models")
                    color: Theme.p.inkPrimary
                    Component.onCompleted: Type.apply(this, Type.h1)
                }
                Text {
                    Layout.fillWidth: true
                    text: ModelStore.activeModelId.length > 0
                          ? qsTr("Selected for the next engine session: %1").arg(ModelStore.activeModelId)
                          : qsTr("Choose an installed model or download a catalogue model.")
                    color: Theme.p.inkSecondary
                    wrapMode: Text.Wrap
                    Component.onCompleted: Type.apply(this, Type.bodySmall)
                }
            }
            GhostButton {
                text: qsTr("Downloads")
                onClicked: IntentDispatcher.dispatch({ type: "navigation.open", route: "downloads" })
            }
        }

        StoragePressureCard { Layout.fillWidth: true }

        Rectangle {
            Layout.fillWidth: true
            visible: ModelStore.restartRequired || ModelStore.sessionMessage.length > 0
            implicitHeight: sessionText.implicitHeight + Spacing.lg * 2
            radius: Theme.radiusLg
            color: Qt.rgba(Theme.p.accent.r, Theme.p.accent.g, Theme.p.accent.b, 0.10)
            border.width: 1
            border.color: Theme.p.accent
            Text {
                id: sessionText
                anchors.fill: parent
                anchors.margins: Spacing.lg
                text: ModelStore.sessionMessage.length > 0
                      ? ModelStore.sessionMessage
                      : qsTr("The selected model will be used after a supported engine session starts.")
                color: Theme.p.inkPrimary
                wrapMode: Text.Wrap
                Component.onCompleted: Type.apply(this, Type.bodyUI)
            }
        }

        ListView {
            id: modelView
            Layout.fillWidth: true
            Layout.fillHeight: true
            model: ModelStore.models
            spacing: Spacing.md
            clip: true
            reuseItems: true
            cacheBuffer: Math.max(height, 700)

            delegate: Rectangle {
                required property string modelId
                required property string displayName
                required property string description
                required property string sizeLabel
                required property real minRamMiB
                required property real contextTokens
                required property bool installed
                required property string downloadState
                required property real progress
                width: ListView.view.width
                implicitHeight: card.implicitHeight + Spacing.lg * 2
                radius: Theme.radiusLg
                color: ModelStore.activeModelId === modelId ? Theme.p.surfaceFaint : Theme.p.surface
                border.width: ModelStore.activeModelId === modelId ? 2 : 1
                border.color: ModelStore.activeModelId === modelId ? Theme.p.accent : Theme.p.divider

                ColumnLayout {
                    id: card
                    anchors.fill: parent
                    anchors.margins: Spacing.lg
                    spacing: Spacing.sm
                    RowLayout {
                        Layout.fillWidth: true
                        MukeiIcon { name: "chip"; size: 24; tone: Theme.p.accent }
                        ColumnLayout {
                            Layout.fillWidth: true
                            spacing: Spacing.xxs
                            Text {
                                Layout.fillWidth: true
                                text: displayName
                                color: Theme.p.inkPrimary
                                wrapMode: Text.Wrap
                                Component.onCompleted: Type.apply(this, Type.h3)
                            }
                            Text {
                                Layout.fillWidth: true
                                text: qsTr("%1 · %2 GB RAM · %3 token context")
                                      .arg(sizeLabel).arg(Math.ceil(minRamMiB / 1024)).arg(contextTokens)
                                color: Theme.p.inkSecondary
                                wrapMode: Text.Wrap
                                Component.onCompleted: Type.apply(this, Type.bodySmall)
                            }
                        }
                        StatusPill {
                            text: ModelStore.activeModelId === modelId ? qsTr("Selected")
                                : installed ? qsTr("Installed")
                                : downloadState === "downloading" ? qsTr("Downloading")
                                : qsTr("Available")
                            subtype: ModelStore.activeModelId === modelId || installed ? "Success"
                                   : downloadState === "failed" ? "Error" : "Neutral"
                        }
                    }
                    Text {
                        Layout.fillWidth: true
                        text: description
                        color: Theme.p.inkSecondary
                        wrapMode: Text.Wrap
                        Component.onCompleted: Type.apply(this, Type.bodyUI)
                    }
                    ProgressBar {
                        Layout.fillWidth: true
                        visible: ["queued", "starting", "downloading", "cancelling"].indexOf(downloadState) >= 0
                        value: progress
                    }
                    RowLayout {
                        Layout.fillWidth: true
                        SecondaryButton {
                            visible: installed && ModelStore.activeModelId !== modelId
                            enabled: CapabilityStore.canSwitchModel && !CapabilityStore.isInferencing
                            text: qsTr("Select")
                            Accessible.description: qsTr("Validate and select this installed model for the next engine session")
                            onClicked: IntentDispatcher.dispatch({ type: "model.select", modelId: modelId })
                        }
                        SecondaryButton {
                            visible: !installed && ["queued", "starting", "downloading", "cancelling"].indexOf(downloadState) < 0
                            enabled: CapabilityStore.canDownloadModel && !StorageStore.critical
                            text: qsTr("Download")
                            onClicked: IntentDispatcher.dispatch({ type: "model.download", modelId: modelId })
                        }
                        GhostButton {
                            visible: ["queued", "starting", "downloading"].indexOf(downloadState) >= 0
                            enabled: CapabilityStore.canStopDownload
                            text: qsTr("Stop")
                            onClicked: IntentDispatcher.dispatch({ type: "download.cancel", modelId: modelId })
                        }
                        DestructiveButton {
                            visible: installed
                            enabled: CapabilityStore.canDeleteModel && !CapabilityStore.isInferencing
                            text: qsTr("Delete")
                            onCommitted: {
                                root.pendingDeleteModelId = modelId
                                root.pendingDeleteModelName = displayName
                                deleteDialog.open()
                            }
                        }
                        Item { Layout.fillWidth: true }
                    }
                }
            }
        }
    }

    Component.onCompleted: IntentDispatcher.dispatch({ type: "models.refresh" })
}
