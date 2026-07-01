import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import com.mukei.theme

Flickable {
    id: root
    property string code: ""
    property string language: ""
    LayoutMirroring.enabled: false
    Accessible.role: Accessible.StaticText
    Accessible.name: qsTr("Code block")
    Accessible.description: language
    implicitHeight: Math.max(Spacing.xxl, codeText.implicitHeight + Spacing.lg)
    contentWidth: codeText.implicitWidth + Spacing.lg
    contentHeight: codeText.implicitHeight + Spacing.lg
    clip: true
    Rectangle { anchors.fill: parent; color: Theme.p.surfaceVariant; radius: Spacing.xs }
    Text { id: codeText; x: Spacing.md; y: Spacing.md; text: root.code; color: Theme.p.inkPrimary; textFormat: Text.PlainText; Component.onCompleted: Type.apply(this, Type.code) }
    CopyButton { anchors.top: parent.top; anchors.right: parent.right; textToCopy: root.code }
}
