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
    implicitWidth: Math.max(Spacing.huge, label.implicitWidth + Spacing.xl)
    implicitHeight: Spacing.xxl

    background: Rectangle {
        radius: Theme.radiusMd
        color: root.enabled ? Theme.p.accent : Theme.p.surfaceVariant
        border.width: Theme.highContrast ? 1 : 0
        border.color: Theme.p.inkPrimary
        Behavior on color {
            enabled: !Theme.reduceMotion
            ColorAnimation { duration: Motion.themeCrossFade; easing.type: Easing.BezierSpline; easing.bezierCurve: Motion.enter }
        }
        scale: tapHandler.pressed ? 0.97 : 1.0
        Behavior on scale {
            enabled: !Theme.reduceMotion
            NumberAnimation { duration: Motion.buttonPressTint; easing.type: Easing.BezierSpline; easing.bezierCurve: Motion.enter }
        }
    }

    contentItem: Text {
        id: label
        text: root.text
        color: Theme.p.background
        horizontalAlignment: Text.AlignHCenter
        verticalAlignment: Text.AlignVCenter
        Component.onCompleted: Type.apply(this, Type.bodyUI)
    }

    TapHandler {
        id: tapHandler
        onTapped: root.clicked()
    }
}
