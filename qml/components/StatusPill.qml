import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import "../theme"

Rectangle {
    id: root
    property string text: ""
    property string iconSource: ""
    property string subtype: "ActiveTool"
    Accessible.role: Accessible.StaticText
    Accessible.name: text
    Accessible.description: qsTr("Status %1").arg(subtype)
    radius: height / 2
    color: subtype === "Failure" ? Theme.error : subtype === "Success" || subtype === "Network-Offline" ? Theme.success : Theme.p.surface
    border.width: 1
    border.color: Theme.p.accent
    Behavior on color {
        enabled: !Theme.reduceMotion
        ColorAnimation { duration: Motion.toolCrossFade; easing.type: Easing.OutCubic }
    }
    implicitHeight: Spacing.xl
    implicitWidth: row.implicitWidth + Spacing.md
    RowLayout {
        id: row
        anchors.centerIn: parent
        spacing: Spacing.xs
        Image {
            visible: root.iconSource.length > 0
            source: root.iconSource
            Layout.preferredWidth: Spacing.md
            Layout.preferredHeight: Spacing.md
        }
        Text {
            text: root.text
            color: Theme.p.inkPrimary
            Component.onCompleted: Type.apply(this, Type.caption)
        }
    }
}
