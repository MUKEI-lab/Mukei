import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import "../theme"

Popup {
    id: root
    property string message: ""
    y: Spacing.lg
    modal: false
    focus: false
    closePolicy: Popup.NoAutoClose
    Timer {
        interval: Motion.toastDismiss
        running: root.opened
        onTriggered: root.close()
    }
    background: Rectangle {
        color: Theme.p.surface
        radius: Theme.radiusSm
        border.width: Theme.highContrast ? 1 : 0
        border.color: Theme.p.divider
    }
    contentItem: Text {
        text: root.message
        color: Theme.p.inkPrimary
        font.family: Type.bodyUI.family
        font.pixelSize: Type.bodyUI.pixelSize
        lineHeight: Type.bodyUI.lineHeight
        lineHeightMode: Type.bodyUI.lineHeightMode
    }
}
