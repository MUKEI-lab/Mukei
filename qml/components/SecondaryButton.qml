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
    implicitWidth: Math.max(Spacing.huge, label.implicitWidth + Spacing.xl)
    implicitHeight: Spacing.xxl
    background: Rectangle { radius: Spacing.xs; color: "transparent"; border.width: 1; border.color: Theme.p.accent }
    contentItem: Text { id: label; text: root.text; color: Theme.p.accent; horizontalAlignment: Text.AlignHCenter; verticalAlignment: Text.AlignVCenter; Component.onCompleted: Type.apply(this, Type.bodyUI) }
    TapHandler { onTapped: root.clicked() }
}
