import QtQuick
import QtQuick.Effects
import "../theme"

Item {
    id: root
    property string name: ""
    property int size: Spacing.lg
    property color tone: Theme.p.inkPrimary
    property bool mirrored: false
    implicitWidth: size
    implicitHeight: size

    Image {
        id: sourceImage
        anchors.fill: parent
        source: root.name.length > 0 ? "qrc:/icons/" + root.name + ".svg" : ""
        sourceSize.width: root.size
        sourceSize.height: root.size
        fillMode: Image.PreserveAspectFit
        mirror: root.mirrored
        visible: false
    }

    MultiEffect {
        anchors.fill: sourceImage
        source: sourceImage
        colorization: 1
        colorizationColor: root.tone
    }
}
