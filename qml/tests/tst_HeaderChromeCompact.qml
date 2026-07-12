import QtQuick
import QtTest
import "../components"
import "../theme"

TestCase {
    name: "HeaderChromeCompact"
    width: 360
    height: 200
    when: windowShown

    Component { id: iconFactory; IconButton { text: "Back"; iconSource: "qrc:/icons/back.svg" } }

    function test_header_icon_keeps_touch_target_at_large_type() {
        var old = Theme.scale
        Theme.scale = 2.0
        var icon = createTemporaryObject(iconFactory, this)
        verify(icon.implicitWidth >= 44)
        verify(icon.implicitHeight >= 44)
        verify(Type.compact)
        Theme.scale = old
    }
}
