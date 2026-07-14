pragma ComponentBehavior: Bound
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
                    text: ModelStore.activationInProgress
                          ? qsTr("Activating model: %1").arg(ModelStore.activationModelId || ModelStore.selectedModelId)
                          : ModelStore.activeModelId.length > 0
                            ? qsTr("Active model: %1").arg(ModelStore.activeModelId)
                            : ModelStore.selectedModelId.length > 0
                              ? qsTr("Selected model is not active yet: %1").arg(ModelStore.selectedModelId)
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
            visible: ModelStore.activationInProgress || ModelStore.activationFailed
                     || ModelStore.restartRequired || ModelStore.sessionMessage.length > 0
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
                      : ModelStore.activationInProgress
                        ? qsTr("The selected model is being verified and activated.")
                        : ModelStore.activationFailed
                          ? qsTr("The replacement model could not be activated; the previous ready model remains active when available.")
                          : qsTr("No model backend is active yet.")
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
                id: modelDelegate
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
                color: ModelStore.activeModelId === modelDelegate.modelId ? Theme.p.surfaceFaint : Theme.p.surface
                border.width: ModelStore.activeModelId === modelDelegate.modelId ? 2 : 1
                border.color: ModelStore.activeModelId === modelDelegate.modelId ? Theme.p.accent : Theme.p.divider

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
                                text: modelDelegate.displayName
                                color: Theme.p.inkPrimary
                                wrapMode: Text.Wrap
                                Component.onCompleted: Type.apply(this, Type.h3)
                            }
                            Text {
                                Layout.fillWidth: true
                                text: qsTr("%1 · %2 GB RAM · %3 token context")
                                      .arg(modelDelegate.sizeLabel).arg(Math.ceil(modelDelegate.minRamMiB / 1024)).arg(modelDelegate.contextTokens)
                                color: Theme.p.inkSecondary
                                wrapMode: Text.Wrap
                                Component.onCompleted: Type.apply(this, Type.bodySmall)
                            }
                        }
                        StatusPill {
                            text: ModelStore.activeModelId === modelDelegate.modelId ? qsTr("Active")
                                : modelDelegate.installed ? qsTr("Installed")
                                : modelDelegate.downloadState === "downloading" ? qsTr("Downloading")
                                : qsTr("Available")
                            subtype: ModelStore.activeModelId === modelDelegate.modelId || modelDelegate.installed ? "Success"
                                   : modelDelegate.downloadState === "failed" ? "Error" : "Neutral"
                        }
                    }
                    Text {
                        Layout.fillWidth: true
                        text: modelDelegate.description
                        color: Theme.p.inkSecondary
                        wrapMode: Text.Wrap
                        Component.onCompleted: Type.apply(this, Type.bodyUI)
                    }
                    ProgressBar {
                        Layout.fillWidth: true
                        visible: ["queued", "starting", "downloading", "cancelling"].indexOf(modelDelegate.downloadState) >= 0
                        value: modelDelegate.progress
                    }
                    RowLayout {
                        Layout.fillWidth: true
                        SecondaryButton {
                            visible: modelDelegate.installed && ModelStore.activeModelId !== modelDelegate.modelId
                            enabled: CapabilityStore.canSwitchModel && !CapabilityStore.isInferencing
                                     && !ModelStore.activationInProgress
                            text: ModelStore.activationInProgress && ModelStore.activationModelId === modelDelegate.modelId
                                  ? qsTr("Activating…") : qsTr("Activate")
                            Accessible.description: qsTr("Verify and activate this installed model")
                            onClicked: IntentDispatcher.dispatch({ type: "model.select", modelId: modelDelegate.modelId })
                        }
                        SecondaryButton {
                            visible: !modelDelegate.installed && ["queued", "starting", "downloading", "cancelling"].indexOf(modelDelegate.downloadState) < 0
                            enabled: CapabilityStore.canDownloadModel && !StorageStore.critical
                            text: qsTr("Download")
                            onClicked: IntentDispatcher.dispatch({ type: "model.download", modelId: modelDelegate.modelId })
                        }
                        GhostButton {
                            visible: ["queued", "starting", "downloading"].indexOf(modelDelegate.downloadState) >= 0
                            enabled: CapabilityStore.canStopDownload
                            text: qsTr("Stop")
                            onClicked: IntentDispatcher.dispatch({ type: "download.cancel", modelId: modelDelegate.modelId })
                        }
                        DestructiveButton {
                            visible: modelDelegate.installed
                            enabled: CapabilityStore.canDeleteModel && !CapabilityStore.isInferencing
                                     && !ModelStore.activationInProgress
                                     && ModelStore.activeModelId !== modelDelegate.modelId
                            text: qsTr("Delete")
                            onCommitted: {
                                root.pendingDeleteModelId = modelDelegate.modelId
                                root.pendingDeleteModelName = modelDelegate.displayName
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
