import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import "../theme"
import "../components"

Page {
    id: root
    signal modelChosen(string modelId)
    background: Rectangle { color: Theme.p.background }
    Accessible.role: Accessible.Pane
    Accessible.name: qsTr("Choose a model")

    ColumnLayout {
        anchors.fill: parent
        anchors.margins: ResponsiveStore.compact ? Spacing.md : Spacing.xl
        spacing: Spacing.lg
        RowLayout {
            Layout.fillWidth: true
            IconButton {
                iconSource: "qrc:/icons/back.svg"
                text: qsTr("Back")
                onClicked: IntentDispatcher.dispatch({ type: "navigation.back" })
            }
            Text {
                Layout.fillWidth: true
                text: qsTr("Choose a model")
                color: Theme.p.inkPrimary
                Component.onCompleted: Type.apply(this, Type.h1)
            }
        }
        ListView {
            Layout.fillWidth: true
            Layout.fillHeight: true
            model: ModelStore.models
            spacing: Spacing.md
            clip: true
            delegate: Rectangle {
                required property string modelId
                required property string displayName
                required property string description
                required property string sizeLabel
                required property bool installed
                width: ListView.view.width
                implicitHeight: pickerContent.implicitHeight + Spacing.lg * 2
                radius: Theme.radiusLg
                color: Theme.p.surface
                border.width: 1
                border.color: Theme.p.divider
                ColumnLayout {
                    id: pickerContent
                    anchors.fill: parent
                    anchors.margins: Spacing.lg
                    Text {
                        Layout.fillWidth: true
                        text: displayName
                        color: Theme.p.inkPrimary
                        wrapMode: Text.Wrap
                        Component.onCompleted: Type.apply(this, Type.h3)
                    }
                    Text {
                        text: sizeLabel
                        color: Theme.p.inkSecondary
                        Component.onCompleted: Type.apply(this, Type.bodySmall)
                    }
                    Text {
                        Layout.fillWidth: true
                        text: description
                        color: Theme.p.inkSecondary
                        wrapMode: Text.Wrap
                        Component.onCompleted: Type.apply(this, Type.bodyUI)
                    }
                    SecondaryButton {
                        text: installed ? qsTr("Installed") : qsTr("Download")
                        enabled: !installed && CapabilityStore.canDownloadModel && !StorageStore.critical
                        onClicked: IntentDispatcher.dispatch({ type: "model.download", modelId: modelId })
                    }
                }
            }
        }
    }
    Component.onCompleted: IntentDispatcher.dispatch({ type: "models.refresh" })
}
