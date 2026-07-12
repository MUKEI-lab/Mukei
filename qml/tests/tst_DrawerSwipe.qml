import QtQuick
import QtQuick.Controls
import QtTest

TestCase {
    name: "DrawerSwipe"
    width: 360
    height: 640
    when: windowShown

    Drawer { id: drawer; width: 280; edge: Qt.LeftEdge }

    function test_drawer_uses_leading_edge_and_bounded_width() {
        compare(drawer.edge, Qt.LeftEdge)
        verify(drawer.width >= 240)
        verify(drawer.width < width)
    }
}
