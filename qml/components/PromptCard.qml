import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import "../theme"

Rectangle {
    id: root
    property string prompt: ""
    property bool promptCardAutoSend: false
    signal fillRequested(string prompt)
    signal sendRequested(string prompt)
    Accessible.role: Accessible.Button
    Accessible.name: qsTr("Fill prompt")
    Accessible.description: prompt
    radius: Theme.radiusSm
    color: Theme.p.surface
    implicitHeight: promptText.implicitHeight + Spacing.lg
    Text {
        id: promptText
        anchors.fill: parent
        anchors.margins: Spacing.md
        text: root.prompt
        color: Theme.p.inkPrimary
        wrapMode: Text.Wrap
        Component.onCompleted: Type.apply(this, Type.bodySmallItalic)
    }
    TapHandler {
        onTapped: root.promptCardAutoSend ? root.sendRequested(root.prompt) : root.fillRequested(root.prompt)
    }
}
