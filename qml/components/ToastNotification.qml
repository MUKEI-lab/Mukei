import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import QtQuick.Accessibility
import com.mukei.theme
Item { id: root; property string title: "ToastNotification"; property string text: ""; implicitWidth: 320; implicitHeight: 56; Accessible.role: Accessible.StaticText; Accessible.name: title; Rectangle { anchors.fill: parent; radius: Theme.radiusMd; color: Theme.p.surface; border.color: Theme.p.surfaceVariant } Text { anchors.centerIn: parent; text: root.text.length ? root.text : root.title; color: Theme.p.inkPrimary; font: Type.bodyUI } }
