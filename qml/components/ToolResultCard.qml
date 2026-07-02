import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import "../theme"

Rectangle {
    id: root
    property string title: qsTr("Tool result")
    property string body: ""
    Accessible.role: Accessible.StaticText
    Accessible.name: title
    Accessible.description: body
    radius: Theme.radiusSm
    color: Theme.p.surface
    border.width: 1
    border.color: Theme.p.divider
    Behavior on color {
        enabled: !Theme.reduceMotion
        ColorAnimation { duration: Motion.themeCrossFade; easing.type: Easing.BezierSpline; easing.bezierCurve: Motion.enter }
    }
    Behavior on border.color {
        enabled: !Theme.reduceMotion
        ColorAnimation { duration: Motion.themeCrossFade; easing.type: Easing.BezierSpline; easing.bezierCurve: Motion.enter }
    }
    implicitHeight: column.implicitHeight + Spacing.md * 2
    ColumnLayout {
        id: column
        anchors.fill: parent
        anchors.margins: Spacing.md
        Text {
            text: root.title
            color: Theme.p.inkPrimary
            font.family: Type.h3.family
            font.pixelSize: Type.h3.pixelSize
            lineHeight: Type.h3.lineHeight
            lineHeightMode: Type.h3.lineHeightMode
        }
        Text {
            text: root.body
            color: Theme.p.inkSecondary
            wrapMode: Text.Wrap
            font.family: Type.code.family
            font.pixelSize: Type.code.pixelSize
            lineHeight: Type.code.lineHeight
            lineHeightMode: Type.code.lineHeightMode
        }
    }
}
