import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import "../theme"

Control {
    id: root
    property real value: 0
    Accessible.role: Accessible.ProgressBar
    Accessible.name: qsTr("Progress")
    Accessible.description: qsTr("Progress value")
    implicitHeight: Spacing.xxs
    background: Rectangle {
        color: Theme.p.surfaceVariant
        radius: Theme.radiusXxs
    }
    contentItem: Rectangle {
        width: parent.width * Math.max(0, Math.min(1, root.value))
        height: parent.height
        radius: Theme.radiusXxs
        color: Theme.p.accent
        Behavior on width {
            enabled: !Theme.reduceMotion
            NumberAnimation {
                duration: Motion.progressValue
                easing.type: Easing.BezierSpline
                easing.bezierCurve: Motion.enter
            }
        }
    }
}
