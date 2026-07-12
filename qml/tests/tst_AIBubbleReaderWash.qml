import QtQuick
import QtTest
import "../components"
import "../theme"

TestCase {
    name: "AIBubbleReaderWash"
    width: 480
    height: 300
    when: windowShown

    Component { id: bubbleFactory; AIMessageBubble { width: 420; text: "Answer" } }

    function test_code_content_enables_reader_wash() {
        var bubble = createTemporaryObject(bubbleFactory, this)
        verify(bubble !== null)
        bubble.containsCodeBlock = true
        verify(bubble.readerWash)
        compare(bubble.Accessible.name, "Mukei response")
    }

    function test_large_type_enables_reader_wash() {
        var old = Theme.scale
        var bubble = createTemporaryObject(bubbleFactory, this)
        Theme.scale = 1.75
        tryCompare(bubble, "readerWash", true)
        Theme.scale = old
    }
}
