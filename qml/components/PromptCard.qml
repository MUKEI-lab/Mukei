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
        text: root.prompt
        color: Theme.p.inkPrimary
        wrapMode: Text.Wrap
        verticalAlignment: Text.AlignVCenter
        leftPadding: Spacing.md
        rightPadding: Spacing.md
        topPadding: Spacing.md
        bottomPadding: Spacing.md
        Component.onCompleted: Type.apply(this, Type.bodySmallItalic)
    }

    onClicked: root.promptCardAutoSend
               ? root.sendRequested(root.prompt)
               : root.fillRequested(root.prompt)
}
