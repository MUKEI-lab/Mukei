import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import QtQuick.Accessibility
import com.mukei.theme
Control { id: root; property string text: ""; property string iconSource: ""; signal clicked(); Accessible.role: Accessible.Button; Accessible.name: text; implicitHeight: 48; implicitWidth: Math.max(96, label.implicitWidth + 32); Rectangle { anchors.fill: parent; radius: Theme.radiusMd; color: Theme.p.accent } Text { id: label; anchors.centerIn: parent; text: root.text; color: Theme.p.background; font: Type.bodyUI } TapHandler { onTapped: root.clicked() } }
