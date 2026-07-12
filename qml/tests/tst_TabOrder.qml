import QtQuick
import QtTest
import "../components"

TestCase {
    name: "TabOrder"
    width: 480
    height: 240
    when: windowShown

    Item {
        id: fixture
        anchors.fill: parent
        PrimaryButton { id: first; text: "First"; KeyNavigation.tab: second }
        PrimaryButton { id: second; text: "Second"; anchors.top: first.bottom; KeyNavigation.backtab: first }
    }

    function test_controls_are_keyboard_focusable() {
        verify(first.activeFocusOnTab)
        verify(second.activeFocusOnTab)
        compare(first.KeyNavigation.tab, second)
        compare(second.KeyNavigation.backtab, first)
    }
}
