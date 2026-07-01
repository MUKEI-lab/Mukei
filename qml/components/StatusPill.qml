import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import QtQuick.Accessibility
import com.mukei.theme
Rectangle { id: root; property string text: ""; property string iconSource: ""; property string subtype: "ActiveTool"; radius: height/2; color: subtype === "Failure" ? Theme.p.danger : subtype === "Success" ? Theme.p.success : Theme.p.surface; border.color: Theme.p.accent; implicitHeight: 32; implicitWidth: row.implicitWidth + Spacing.md*2; Row { id: row; anchors.centerIn: parent; spacing: Spacing.xs; Image { visible: root.iconSource.length>0; source: root.iconSource; width: 16; height: 16 } Text { text: root.text; color: Theme.p.inkPrimary; font: Type.caption } } }
