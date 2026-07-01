import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import "../theme"

QtObject {
    id: root
    signal allLoaded
    readonly property var fonts: ["qrc:/fonts/PlayfairDisplay-Regular.ttf", "qrc:/fonts/PlayfairDisplay-Medium.ttf", "qrc:/fonts/PlayfairDisplay-SemiBold.ttf", "qrc:/fonts/PlayfairDisplay-Bold.ttf", "qrc:/fonts/Merriweather-Regular.ttf", "qrc:/fonts/Merriweather-Italic.ttf", "qrc:/fonts/Merriweather-Bold.ttf", "qrc:/fonts/Merriweather-BoldItalic.ttf", "qrc:/fonts/Inter-Regular.ttf", "qrc:/fonts/Inter-Medium.ttf", "qrc:/fonts/Inter-SemiBold.ttf", "qrc:/fonts/JetBrainsMono-Regular.ttf", "qrc:/fonts/JetBrainsMono-Medium.ttf", "qrc:/fonts/JetBrainsMono-Bold.ttf"]
    Component.onCompleted: root.allLoaded()
}
