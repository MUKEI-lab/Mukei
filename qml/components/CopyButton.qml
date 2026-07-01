import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import "../theme"

// Copy-to-clipboard button. On tap: writes `textToCopy` to Qt's system
// clipboard via the C++ `mukeiClipboard.setText` invokable, then
// flashes the icon `check.svg` for `Motion.caretDoneSwap` ms as visual
// acknowledgement.
IconButton {
    id: root
    property string textToCopy: ""
    property bool _acknowledging: false
    iconSource: _acknowledging ? "qrc:/icons/check.svg" : "qrc:/icons/copy.svg"
    Accessible.name: qsTr("Copy text")
    Accessible.description: qsTr("Copy text to the clipboard")

    onClicked: {
        mukeiClipboard.setText(root.textToCopy);
        root._acknowledging = true;
        _ackTimer.restart();
    }

    Timer {
        id: _ackTimer
        interval: Motion.caretDoneSwap
        onTriggered: root._acknowledging = false
    }
}
