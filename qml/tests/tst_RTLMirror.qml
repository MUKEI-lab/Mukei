import QtQuick
import QtTest

TestCase {
    name: "RTLMirror"

    Item {
        id: mirrored
        LayoutMirroring.enabled: true
        LayoutMirroring.childrenInherit: true
        Item { id: child }
    }

    function test_children_inherit_mirroring() {
        verify(mirrored.LayoutMirroring.enabled)
        verify(mirrored.LayoutMirroring.childrenInherit)
        verify(child.LayoutMirroring.enabled)
    }
}
