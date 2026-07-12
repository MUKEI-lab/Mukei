import QtQuick
import QtTest
import "../components"

TestCase {
    name: "BubbleFooterDensity"
    width: 480
    height: 280
    when: windowShown

    Component {
        id: bubbleFactory
        AIMessageBubble {
            width: 420
            text: "A compact answer"
            timestamp: "Now"
            suggestedAction: "Retry"
        }
    }

    function test_footer_does_not_collapse_content() {
        var bubble = createTemporaryObject(bubbleFactory, this)
        verify(bubble !== null)
        verify(bubble.implicitHeight > 44)
        compare(bubble.timestamp, "Now")
        compare(bubble.suggestedAction, "Retry")
    }
}
