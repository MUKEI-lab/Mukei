import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import com.mukei.theme

Text {
    id: root
    property bool finalized: false
    text: finalized ? qsTr("🎯") : qsTr("▌")
    color: Theme.p.accent
    Accessible.role: Accessible.StaticText
    Accessible.name: finalized ? qsTr("Response complete") : qsTr("Mukei is typing")
    Accessible.description: qsTr("Streaming caret")
    Component.onCompleted: Type.apply(this, Type.bodyUI)
    SequentialAnimation on opacity { running: !root.finalized && !Theme.reduceMotion; loops: Animation.Infinite; NumberAnimation { to: 0.5; duration: Motion.toolPulse / 2 } NumberAnimation { to: 1; duration: Motion.toolPulse / 2 } }
}
