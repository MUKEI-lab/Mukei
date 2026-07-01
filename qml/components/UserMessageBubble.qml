import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import com.mukei.theme

Item {
    id: root
    property string text: ""
    property string timestamp: ""
    signal actionRequested(string action)
    Accessible.role: Accessible.StaticText
    Accessible.name: qsTr("User message")
    Accessible.description: text
    implicitHeight: column.implicitHeight
    Layout.alignment: Qt.AlignRight
    ColumnLayout { id: column; anchors.right: parent.right; width: Math.min(parent ? parent.width * 0.78 : Spacing.huge * 3, bubble.implicitWidth); spacing: Spacing.xxs; Rectangle { id: bubble; Layout.fillWidth: true; implicitHeight: message.implicitHeight + Spacing.md * 2; radius: Spacing.sm; color: Theme.p.surfaceVariant; Text { id: message; anchors.fill: parent; anchors.margins: Spacing.md; text: root.text; wrapMode: Text.Wrap; color: Theme.p.inkPrimary; Component.onCompleted: Type.apply(this, Type.bodyUI) } } Text { text: root.timestamp; color: Theme.p.inkFaint; Component.onCompleted: Type.apply(this, Type.caption) } }
    TapHandler { acceptedButtons: Qt.RightButton; onTapped: root.actionRequested("Edit") }
}
