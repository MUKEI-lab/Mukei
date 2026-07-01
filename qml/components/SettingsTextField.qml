import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import com.mukei.theme

Control {
    id: root
    property alias text: input.text
    property string label: ""
    property string errorText: ""
    Accessible.role: Accessible.EditableText
    Accessible.name: label
    Accessible.description: errorText.length > 0 ? errorText : qsTr("Editable setting")
    implicitHeight: column.implicitHeight
    implicitWidth: Spacing.huge * 3
    contentItem: ColumnLayout {
        id: column
        spacing: Spacing.xs
        Text { text: root.label; color: Theme.p.inkSecondary; Component.onCompleted: Type.apply(this, Type.caption) }
        TextField { id: input; Layout.fillWidth: true; color: Theme.p.inkPrimary; placeholderTextColor: Theme.p.inkFaint; echoMode: root.label.toLowerCase().indexOf("key") >= 0 ? TextInput.Password : TextInput.Normal; background: Rectangle { color: Theme.p.surface; border.width: 1; border.color: root.errorText.length > 0 ? Theme.error : Theme.p.divider; radius: Spacing.xs } }
        Text { visible: root.errorText.length > 0; text: root.errorText; color: Theme.error; Component.onCompleted: Type.apply(this, Type.caption) }
    }
}
