import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import com.mukei.theme

Rectangle {
    id: root
    property string title: qsTr("Tool result")
    property string body: ""
    Accessible.role: Accessible.StaticText
    Accessible.name: title
    Accessible.description: body
    radius: Spacing.sm
    color: Theme.p.surface
    border.width: 1
    border.color: Theme.p.divider
    implicitHeight: column.implicitHeight + Spacing.md * 2
    ColumnLayout { id: column; anchors.fill: parent; anchors.margins: Spacing.md; Text { text: root.title; color: Theme.p.inkPrimary; Component.onCompleted: Type.apply(this, Type.h3) } Text { text: root.body; color: Theme.p.inkSecondary; wrapMode: Text.Wrap; Component.onCompleted: Type.apply(this, Type.code) } }
}
