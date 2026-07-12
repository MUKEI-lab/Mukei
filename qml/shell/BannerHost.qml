import QtQuick
import QtQuick.Layouts
import "../theme"

import "../stores"
Item {
    id: root
    implicitHeight: banner.visible ? banner.implicitHeight : 0

    Rectangle {
        id: banner
        anchors.left: parent.left
        anchors.right: parent.right
        visible: LifecycleStore.degraded || LifecycleStore.quarantined || StorageStore.warning
        implicitHeight: message.implicitHeight + Spacing.sm * 2
        color: LifecycleStore.quarantined || StorageStore.critical ? Theme.error : Theme.warning
        opacity: 0.94

        Text {
            id: message
            anchors.fill: parent
            anchors.margins: Spacing.sm
            text: LifecycleStore.degraded || LifecycleStore.quarantined
                  ? LifecycleStore.description
                  : StorageStore.critical
                    ? qsTr("Storage is almost full. Downloads and indexing may be unavailable.")
                    : qsTr("Storage is getting full. Review local models and partial downloads.")
            color: "white"
            horizontalAlignment: Text.AlignHCenter
            wrapMode: Text.Wrap
            Component.onCompleted: Type.apply(this, Type.bodySmall)
        }
    }
}
