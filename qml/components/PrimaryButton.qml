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

    background: Rectangle {
        radius: Spacing.xs
        color: root.enabled ? Theme.p.accent : Theme.p.surfaceVariant
        border.width: Theme.highContrast ? 1 : 0
        border.color: Theme.p.inkPrimary
    }

    contentItem: Text {
        id: label
        text: root.text
        color: Theme.p.background
        horizontalAlignment: Text.AlignHCenter
        verticalAlignment: Text.AlignVCenter
        Component.onCompleted: Type.apply(this, Type.bodyUI)
    }

    TapHandler { onTapped: root.clicked() }
}
