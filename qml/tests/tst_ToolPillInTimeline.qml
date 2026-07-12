import QtQuick
import QtTest
import "../components"

TestCase {
    name: "ToolPillInTimeline"
    width: 420
    height: 180
    when: windowShown

    Component {
        id: eventFactory
        ChatTimelineEvent {
            width: 380
            kind: "tool"
            phase: "result"
            label: "Search completed"
            toolId: "tool-1"
            iconSource: "qrc:/icons/search.svg"
        }
    }

    function test_tool_result_has_accessible_timeline_semantics() {
        var event = createTemporaryObject(eventFactory, this)
        verify(event !== null)
        compare(event.Accessible.name, "Search completed")
        compare(event.Accessible.description, "Inline tool event")
        verify(event.implicitHeight > 0)
    }
}
