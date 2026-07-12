import QtQuick
import QtTest
import "../components"

TestCase {
    name: "DestructiveConfirmTwoTap"
    width: 320
    height: 180
    when: windowShown

    Component {
        id: fixtureFactory
        Item {
            property alias button: button
            property alias spy: spy
            DestructiveButton { id: button; text: "Delete" }
            SignalSpy { id: spy; target: button; signalName: "committed" }
        }
    }

    function test_first_click_arms_second_click_commits() {
        var fixture = createTemporaryObject(fixtureFactory, this)
        fixture.button.clicked()
        verify(fixture.button.armed)
        compare(fixture.spy.count, 0)
        fixture.button.clicked()
        verify(!fixture.button.armed)
        compare(fixture.spy.count, 1)
    }
}
