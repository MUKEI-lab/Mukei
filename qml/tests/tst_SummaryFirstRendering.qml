import QtQuick
import QtTest
import "../components"

TestCase {
    name: "SummaryFirstRendering"
    width: 420
    height: 240
    when: windowShown

    Component { id: thinkingFactory; ThinkingAccordion { width: 380; text: "Detailed reasoning summary" } }

    function test_thinking_starts_collapsed() {
        var item = createTemporaryObject(thinkingFactory, this)
        verify(item !== null)
        verify(!item.expanded)
        compare(item.Accessible.name, "Expand thinking")
        item.expanded = true
        compare(item.Accessible.name, "Collapse thinking")
    }
}
