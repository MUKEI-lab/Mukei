import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import com.mukei.theme

Popup {
    id: root
    property string message: ""
    y: Spacing.lg
    modal: false
    focus: false
    closePolicy: Popup.NoAutoClose
    Accessible.role: Accessible.AlertMessage
    Accessible.name: message
    Accessible.description: qsTr("Toast notification")
    Timer { interval: Motion.toastDismiss; running: root.opened; onTriggered: root.close() }
    background: Rectangle { color: Theme.p.surface; radius: Spacing.sm; border.width: Theme.highContrast ? 1 : 0; border.color: Theme.p.divider }
    contentItem: Text { text: root.message; color: Theme.p.inkPrimary; Component.onCompleted: Type.apply(this, Type.bodyUI) }
}
