import QtQuick
import QtQuick.Controls.Basic
import "../theme"

Button {
    id: root
    property bool active: false
    Accessible.role: Accessible.Button
    Accessible.name: text
    activeFocusOnTab: true
    implicitWidth: Math.max(44, contentItem.implicitWidth + Spacing.md)
    implicitHeight: Math.max(44, Spacing.xxl)
    background: Rectangle {
        radius: Theme.radiusMd
        color: root.down || root.hovered || root.active ? Theme.p.surfaceFaint : "transparent"
        border.width: root.visualFocus ? 1 : 0
        border.color: Theme.p.accent
        Behavior on color { ColorAnimation { duration: Theme.reduceMotion ? 0 : Motion.microTransition } }
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
