import QtQuick
import QtTest
import "../components"

TestCase {
    name: "PromptCardFillOnly"
    width: 420
    height: 180
    when: windowShown

    Component {
        id: fixtureFactory
        Item {
            width: 380
            height: 120
            property alias card: card
            property alias fillSpy: fillSpy
            property alias sendSpy: sendSpy
            PromptCard { id: card; anchors.fill: parent; prompt: "Explain entropy" }
            SignalSpy { id: fillSpy; target: card; signalName: "fillRequested" }
            SignalSpy { id: sendSpy; target: card; signalName: "sendRequested" }
        }
    }

    function test_default_tap_fills_without_auto_send() {
        var fixture = createTemporaryObject(fixtureFactory, this)
        mouseClick(fixture.card, fixture.card.width / 2, fixture.card.height / 2)
        compare(fixture.fillSpy.count, 1)
        compare(fixture.sendSpy.count, 0)
    }
}
