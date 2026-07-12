import QtQuick
import QtQuick.Controls.Basic
import "../theme"

Button {
    id: root
    Accessible.role: Accessible.Button
    Accessible.name: text
    activeFocusOnTab: true
    implicitWidth: Math.max(Spacing.huge, contentItem.implicitWidth + Spacing.xl)
    implicitHeight: Math.max(44, Spacing.xxl)
    background: Rectangle {
        radius: Theme.radiusMd
        color: root.down ? Theme.p.surfaceFaint : "transparent"
        border.width: root.visualFocus ? 2 : 1
        border.color: Theme.p.accent
        scale: root.down && !Theme.reduceMotion ? 0.98 : 1
        Behavior on scale { NumberAnimation { duration: Motion.immediateFeedback; easing.type: Easing.OutCubic } }
    }
    contentItem: Text {
        text: root.text
        color: root.enabled ? Theme.p.accent : Theme.p.inkFaint
        horizontalAlignment: Text.AlignHCenter
        verticalAlignment: Text.AlignVCenter
        elide: Text.ElideRight
        Component.onCompleted: Type.apply(this, Type.bodyUI)
    }
}
