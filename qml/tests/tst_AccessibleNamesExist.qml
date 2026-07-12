import QtQuick
import QtTest
import "../components"

TestCase {
    name: "AccessibleNamesExist"
    width: 360
    height: 240
    when: windowShown

    Component { id: primaryFactory; PrimaryButton { text: "Save" } }
    Component { id: iconFactory; IconButton { text: "Settings"; iconSource: "qrc:/icons/settings.svg" } }

    function test_primary_button_name() {
        var button = createTemporaryObject(primaryFactory, this)
        verify(button !== null)
        compare(button.Accessible.name, "Save")
        verify(button.implicitHeight >= 44)
    }

    function test_icon_button_name() {
        var button = createTemporaryObject(iconFactory, this)
        verify(button !== null)
        compare(button.Accessible.name, "Settings")
        verify(button.implicitWidth >= 44)
        verify(button.implicitHeight >= 44)
    }
}
