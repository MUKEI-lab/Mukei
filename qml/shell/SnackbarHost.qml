import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import "../theme"

import "../stores"
Item {
    id: root
    implicitHeight: toast.visible ? toast.implicitHeight + Spacing.lg : 0

    Rectangle {
        id: toast
        anchors.horizontalCenter: parent.horizontalCenter
        anchors.bottom: parent.bottom
        anchors.bottomMargin: Spacing.lg
        width: Math.min(parent.width - Spacing.lg * 2, Spacing.huge * 6)
        implicitHeight: row.implicitHeight + Spacing.md * 2
        radius: Theme.radiusLg
        color: Theme.p.surface
        border.width: Theme.highContrast ? 1 : 0
        border.color: Theme.p.divider
        visible: ErrorStore.hasError && ErrorStore.presentation !== "blocking"

        RowLayout {
            id: row
            anchors.fill: parent
            anchors.margins: Spacing.md
            spacing: Spacing.sm

            Text {
                Layout.fillWidth: true
                text: ErrorStore.hasError ? ErrorStore.currentError.safeMessage : ""
                color: Theme.p.inkPrimary
                wrapMode: Text.Wrap
                Component.onCompleted: Type.apply(this, Type.bodySmall)
            }
            ToolButton {
                text: qsTr("Dismiss")
                onClicked: ErrorStore.dismiss()
            }
        }
    }

    Timer {
        interval: 5000
        running: toast.visible
        repeat: false
        onTriggered: ErrorStore.dismiss()
    }
}
