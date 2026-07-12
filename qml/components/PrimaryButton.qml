import QtQuick
import QtQuick.Controls.Basic
import "../theme"

Button {
    id: root
    Accessible.role: Accessible.Button
    Accessible.name: text
    Accessible.description: qsTr("Activate %1").arg(text)
    activeFocusOnTab: true
    implicitWidth: Math.max(Spacing.huge, contentItem.implicitWidth + Spacing.xl)
    implicitHeight: Math.max(44, Spacing.xxl)

    background: Rectangle {
        radius: Theme.radiusMd
        color: root.enabled ? Theme.p.accent : Theme.p.surfaceVariant
        border.width: Theme.highContrast || root.visualFocus ? 1 : 0
        border.color: root.visualFocus ? Theme.p.inkPrimary : Theme.p.divider
        scale: root.down && !Theme.reduceMotion ? 0.975 : 1
        Behavior on scale { NumberAnimation { duration: Motion.immediateFeedback; easing.type: Easing.OutCubic } }
        Behavior on color { ColorAnimation { duration: Theme.reduceMotion ? 0 : Motion.microTransition } }
    }

    contentItem: Text {
        text: root.text
        color: root.enabled ? Theme.p.background : Theme.p.inkFaint
        horizontalAlignment: Text.AlignHCenter
        verticalAlignment: Text.AlignVCenter
        elide: Text.ElideRight
        Component.onCompleted: Type.apply(this, Type.bodyUI)
    }
}
