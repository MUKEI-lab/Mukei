import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import com.mukei.theme

QtObject {
    id: root
    enum Level { Light, Medium, Heavy }
    function pulse(level) { return level }
}
