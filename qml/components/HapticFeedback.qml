import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import "../theme"

QtObject {
    id: root
    enum Level {
        Light,
        Medium,
        Heavy
    }
    function pulse(level) {
        return level;
    }
}
