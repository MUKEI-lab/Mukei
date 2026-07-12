import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import "../theme"

Button {
    id: root
    property string prompt: ""
    property bool promptCardAutoSend: false
    signal fillRequested(string prompt)
    signal sendRequested(string prompt)

    padding: 0
    hoverEnabled: true
    Accessible.role: Accessible.Button
    Accessible.name: qsTr("Fill prompt")
    Accessible.description: prompt
    implicitHeight: promptText.implicitHeight + Spacing.lg

    background: Rectangle {
        radius: Theme.radiusLg
        color: Theme.p.surface
    }

    contentItem: Text {
        id: promptText
        leftPadding: Spacing.md
        rightPadding: Spacing.md
        topPadding: Spacing.md
        bottomPadding: Spacing.md
        text: root.prompt
        color: Theme.p.inkPrimary
        wrapMode: Text.Wrap
        verticalAlignment: Text.AlignVCenter
        Component.onCompleted: Type.apply(this, Type.bodySmallItalic)
    }

    onClicked: root.promptCardAutoSend
               ? root.sendRequested(root.prompt)
               : root.fillRequested(root.prompt)
}
