import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import "../theme"

// Copy-to-clipboard button. On tap: writes `textToCopy` to Qt's system
// clipboard via a bridge-injected `mukeiClipboard.setText` invokable, then
// flashes the icon `check.svg` for `Motion.caretDoneSwap` ms as visual
// acknowledgement. Falls back to a no-op if the bridge is absent (bench
// smoke-runs without the CXX-Qt layer).
IconButton {
    id: root
    property string textToCopy: ""
    property bool _acknowledging: false
    iconSource: _acknowledging ? "qrc:/icons/check.svg" : "qrc:/icons/copy.svg"
    Accessible.name: qsTr("Copy text")
    Accessible.description: qsTr("Copy text to the clipboard")

    onClicked: {
        if (typeof mukeiClipboard !== "undefined" && mukeiClipboard && typeof mukeiClipboard.setText === "function") {
            mukeiClipboard.setText(root.textToCopy);
        } else {
            console.warn("CopyButton: mukeiClipboard bridge unavailable, no-op");
        }
        root._acknowledging = true;
        _ackTimer.restart();
    }

    Timer {
        id: _ackTimer
        interval: Motion.caretDoneSwap
        onTriggered: root._acknowledging = false
    }
}
