import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import "../theme"

Control {
    id: root
    property string text: ""
    signal clicked
    Accessible.role: Accessible.Button
    Accessible.name: root.text
    Accessible.description: qsTr("Activate %1").arg(root.text)
    implicitWidth: Math.max(Spacing.xxl, label.implicitWidth + Spacing.md)
    implicitHeight: Spacing.xxl
    background: Rectangle {
        radius: Spacing.xs
        color: (root.hovered || tapHandler.pressed) ? Theme.p.surfaceFaint : "transparent"
        Behavior on color {
            enabled: !Theme.reduceMotion
            ColorAnimation { duration: Motion.buttonPressTint; easing.type: Easing.OutCubic }
        }
    }
    contentItem: Text {
        id: label
        text: root.text
        color: Theme.p.accent
        horizontalAlignment: Text.AlignHCenter
        verticalAlignment: Text.AlignVCenter
        Component.onCompleted: Type.apply(this, Type.bodyUI)
    }
    TapHandler {
        id: tapHandler
        onTapped: root.clicked()
    }
}
