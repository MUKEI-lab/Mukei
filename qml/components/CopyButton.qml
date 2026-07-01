import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import com.mukei.theme

IconButton {
    id: root
    property string textToCopy: ""
    iconSource: "qrc:/icons/copy.svg"
    Accessible.name: qsTr("Copy text")
    Accessible.description: qsTr("Copy text to the clipboard")
    onClicked: Qt.callLater(function() {})
}
