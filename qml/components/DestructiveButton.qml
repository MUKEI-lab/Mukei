import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import "../theme"

Control {
    id: root

    property string text: qsTr("Delete")
    property string confirmText: qsTr("Tap again")
    property bool armed: false
    signal committed

    Accessible.role: Accessible.Button
    Accessible.name: armed ? confirmText : text
    Accessible.description: qsTr("Destructive two-tap confirmation button")
    implicitWidth: Math.max(Spacing.huge, label.implicitWidth + Spacing.xl)
    implicitHeight: Spacing.xxl

    Timer {
        id: disarmTimer
        interval: Motion.destructiveTimeout
        onTriggered: root.armed = false
    }

    background: Rectangle {
        radius: Theme.radiusMd
        color: Theme.error
    }
    contentItem: Text {
        id: label
        text: root.armed ? root.confirmText : root.text
        color: Theme.p.background
        horizontalAlignment: Text.AlignHCenter
        verticalAlignment: Text.AlignVCenter
        // Live bindings for font properties so they react to Theme.scale changes
        font.family: Type.bodyUI.family
        font.pixelSize: Type.bodyUI.pixelSize
        font.weight: Type.bodyUI.weight
        font.italic: Type.bodyUI.italic
        lineHeightMode: Text.ProportionalHeight
        lineHeight: Type.bodyUI.lineHeight
    }

    TapHandler {
        onTapped: {
            if (root.armed) {
                disarmTimer.stop();
                root.armed = false;
                root.committed();
            } else {
                root.armed = true;
                disarmTimer.restart();
            }
        }
    }
}
