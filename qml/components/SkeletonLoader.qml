import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import "../theme"

// Editorial-luxury skeleton: a low-contrast surface tile with a slow shimmer
// gradient sweeping left-to-right. Respects `Theme.reduceMotion` (static tile,
// no sweep). Auto-hides after `Motion.skeletonMaxVisible` so a slow bridge
// never leaves the shell staring at a shimmer indefinitely.
Rectangle {
    id: root
    Accessible.role: Accessible.StaticText
    Accessible.name: qsTr("Loading placeholder")
    Accessible.description: qsTr("Content is loading")
    radius: Theme.radiusSm
    color: Theme.p.surfaceVariant
    opacity: 0.55
    clip: true
    implicitHeight: Spacing.xxl

    // Shimmer highlight
    Rectangle {
        id: shimmer
        visible: !Theme.reduceMotion
        width: parent.width * 0.4
        height: parent.height
        x: -width
        gradient: Gradient {
            orientation: Gradient.Horizontal
            GradientStop { position: 0.0; color: "transparent" }
            GradientStop { position: 0.5; color: Qt.rgba(1, 1, 1, 0.22) }
            GradientStop { position: 1.0; color: "transparent" }
        }
        NumberAnimation on x {
            running: root.visible && !Theme.reduceMotion
            loops: Animation.Infinite
            from: -shimmer.width
            to: root.width + shimmer.width
            duration: 1400
            easing.type: Easing.InOutQuad
        }
    }

    Timer {
        interval: Motion.skeletonMaxVisible
        running: root.visible
        onTriggered: root.visible = false
    }

    Behavior on opacity {
        enabled: !Theme.reduceMotion
        NumberAnimation {
                duration: Motion.themeCrossFade
                easing.type: Easing.BezierSpline
                easing.bezierCurve: Motion.enter
            }
    }
}
