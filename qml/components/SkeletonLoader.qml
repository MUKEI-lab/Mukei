import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import com.mukei.theme

Rectangle {
    id: root
    Accessible.role: Accessible.StaticText
    Accessible.name: qsTr("Loading placeholder")
    Accessible.description: qsTr("Content is loading")
    radius: Spacing.xs
    color: Theme.p.surfaceVariant
    opacity: 0.05
    implicitHeight: Spacing.xxl
    Timer { interval: Motion.skeletonMaxVisible; running: root.visible; onTriggered: root.visible = false }
}
