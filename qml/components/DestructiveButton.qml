import QtQuick
import QtQuick.Controls.Basic
import "../theme"

Button {
    id: root
    property string confirmText: qsTr("Tap again")
    property bool armed: false
    signal committed
    Accessible.role: Accessible.Button
    Accessible.name: armed ? confirmText : text
    Accessible.description: qsTr("Destructive two-step confirmation")
    activeFocusOnTab: true
    implicitWidth: Math.max(Spacing.huge, contentItem.implicitWidth + Spacing.xl)
    implicitHeight: Math.max(44, Spacing.xxl)

    Timer {
        id: disarmTimer
        interval: Motion.destructiveTimeout
        onTriggered: root.armed = false
    }

    onClicked: {
        if (armed) {
            disarmTimer.stop()
            armed = false
            committed()
        } else {
            armed = true
            disarmTimer.restart()
        }
    }

    background: Rectangle {
        radius: Theme.radiusMd
        color: root.down ? Qt.darker(Theme.error, 1.08) : Theme.error
        border.width: root.visualFocus ? 2 : 0
        border.color: "white"
    }
    contentItem: Text {
        text: root.armed ? root.confirmText : root.text
        color: "white"
        horizontalAlignment: Text.AlignHCenter
        verticalAlignment: Text.AlignVCenter
        Component.onCompleted: Type.apply(this, Type.bodyUI)
    }
}
