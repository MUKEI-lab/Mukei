import QtQuick
import QtQuick.Layouts
import "../stores"
import "../theme"

Rectangle {
    id: root
    visible: OperationStore.hasActiveOperations || DownloadStore.activeCount > 0
    width: label.implicitWidth + Spacing.lg * 2
    height: label.implicitHeight + Spacing.sm * 2
    anchors.topMargin: Spacing.md
    anchors.rightMargin: Spacing.md
    radius: Theme.radiusXl
    color: Theme.p.surface
    border.width: Theme.highContrast ? 1 : 0
    border.color: Theme.p.divider

    Text {
        id: label
        anchors.centerIn: parent
        text: DownloadStore.activeCount > 0
              ? qsTr("%1 model download(s)").arg(DownloadStore.activeCount)
              : qsTr("%1 active operation(s)").arg(OperationStore.activeCount)
        color: Theme.p.inkSecondary
        Component.onCompleted: Type.apply(this, Type.caption)
    }
}
