import QtQuick
import QtTest
import "../theme"

TestCase {
    name: "FontScaleExtreme"

    function test_scale_is_clamped_and_compact_chrome_activates() {
        var original = Theme.scale
        Theme.scale = 2.0
        compare(Type.fontScale, 2.0)
        verify(Type.compact)
        verify(Type.bodyUI.pixelSize >= 30)
        Theme.scale = 4.0
        compare(Type.fontScale, 2.0)
        Theme.scale = original
    }
}
