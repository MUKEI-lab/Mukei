import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import QtQuick.Accessibility
import com.mukei.theme
Control { id: root; property string iconSource: ""; signal clicked(); Accessible.role: Accessible.Button; Accessible.name: qsTr("Icon button"); implicitWidth: 48; implicitHeight: 48; background: Rectangle { color: root.hovered ? Theme.p.surfaceVariant : "transparent"; radius: 24; border.color: root.activeFocus ? Theme.p.focusRing : "transparent" } contentItem: Image { source: root.iconSource; sourceSize.width: 22; sourceSize.height: 22; fillMode: Image.PreserveAspectFit } TapHandler { onTapped: root.clicked() } }
