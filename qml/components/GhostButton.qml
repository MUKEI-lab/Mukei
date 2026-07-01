import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import com.mukei.theme

Control {
    id: root
    property string text: ""
    signal clicked()
    Accessible.role: Accessible.Button
    Accessible.name: root.text
    Accessible.description: qsTr("Activate %1").arg(root.text)
    implicitWidth: Math.max(Spacing.xxl, label.implicitWidth + Spacing.md)
    implicitHeight: Spacing.xxl
    background: Rectangle { radius: Spacing.xs; color: root.hovered ? Theme.p.surfaceFaint : "transparent" }
    contentItem: Text { id: label; text: root.text; color: Theme.p.accent; horizontalAlignment: Text.AlignHCenter; verticalAlignment: Text.AlignVCenter; Component.onCompleted: Type.apply(this, Type.bodyUI) }
    TapHandler { onTapped: root.clicked() }
}
