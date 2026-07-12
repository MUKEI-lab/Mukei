import QtQuick
import QtQuick.Layouts
import "../theme"

Rectangle {
    id: root
    implicitHeight: content.implicitHeight + Spacing.lg * 2
    radius: Theme.radiusLg
    color: StorageStore.critical ? Qt.rgba(Theme.error.r, Theme.error.g, Theme.error.b, 0.12)
                                  : Theme.p.surface
    border.width: 1
    border.color: StorageStore.warning ? (StorageStore.critical ? Theme.error : Theme.warning) : Theme.p.divider
    Accessible.role: Accessible.StaticText
    Accessible.name: qsTr("Local storage usage")

    ColumnLayout {
        id: content
        anchors.fill: parent
        anchors.margins: Spacing.lg
        spacing: Spacing.sm
        RowLayout {
            Layout.fillWidth: true
            Text {
                Layout.fillWidth: true
                text: qsTr("Local model storage")
                color: Theme.p.inkPrimary
                Component.onCompleted: Type.apply(this, Type.h3)
            }
            Text {
                text: qsTr("%1 of %2").arg(StorageStore.formatBytes(StorageStore.accountedModelBytes))
                        .arg(StorageStore.formatBytes(StorageStore.maxModelStorageBytes))
                color: Theme.p.inkSecondary
                Component.onCompleted: Type.apply(this, Type.caption)
            }
        }
        ProgressBar {
            Layout.fillWidth: true
            value: StorageStore.usageRatio
        }
        Text {
            Layout.fillWidth: true
            visible: StorageStore.warning
            text: StorageStore.critical
                  ? qsTr("Storage is nearly full. New downloads and indexing may be blocked.")
                  : qsTr("Storage is filling up. Removing unused models can keep Mukei responsive.")
            color: StorageStore.critical ? Theme.error : Theme.p.inkSecondary
            wrapMode: Text.Wrap
            Component.onCompleted: Type.apply(this, Type.bodySmall)
        }
    }
}
