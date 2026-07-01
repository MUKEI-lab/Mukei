import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import com.mukei.theme

Control {
    id: root

    property string text: qsTr("Delete")
    property string confirmText: qsTr("Tap again")
    property bool armed: false
    signal committed()

    Accessible.role: Accessible.Button
    Accessible.name: armed ? confirmText : text
    Accessible.description: qsTr("Destructive two-tap confirmation button")
    implicitWidth: Math.max(Spacing.huge, label.implicitWidth + Spacing.xl)
    implicitHeight: Spacing.xxl

    Timer { id: disarmTimer; interval: Motion.destructiveTimeout; onTriggered: root.armed = false }

    background: Rectangle { radius: Spacing.xs; color: Theme.error }
    contentItem: Text { id: label; text: root.armed ? root.confirmText : root.text; color: Theme.p.background; horizontalAlignment: Text.AlignHCenter; verticalAlignment: Text.AlignVCenter; Component.onCompleted: Type.apply(this, Type.bodyUI) }

    TapHandler {
        onTapped: {
            if (root.armed) {
                disarmTimer.stop()
                root.armed = false
                root.committed()
            } else {
                root.armed = true
                disarmTimer.restart()
            }
        }
    }
}
