import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import com.mukei.theme

Item {
    id: root
    Accessible.role: Accessible.StaticText
    Accessible.name: qsTr("Loading")
    Accessible.description: qsTr("Progress spinner")
    implicitWidth: Spacing.xl
    implicitHeight: Spacing.xl
    Rectangle { anchors.fill: parent; radius: width / 2; color: "transparent"; border.width: 2; border.color: Theme.p.accent; opacity: Theme.reduceMotion ? 0.6 : 1 }
    RotationAnimator on rotation { running: visible && !Theme.reduceMotion; loops: Animation.Infinite; from: 0; to: 360; duration: 1200 }
}
