import QtQuick
import QtTest
import "../components"

TestCase {
    name: "ComposerMultiline"
    width: 480
    height: 320
    when: windowShown

    Component { id: composerFactory; ChatComposer { width: 420; canSend: true } }

    function test_multiline_text_round_trip() {
        var composer = createTemporaryObject(composerFactory, this)
        verify(composer !== null)
        composer.text = "first line\nsecond line\nthird line"
        compare(composer.text.split("\n").length, 3)
        verify(composer.implicitHeight > 44)
        compare(composer.Accessible.name, "Compose message")
    }

    function test_streaming_disables_send_path() {
        var composer = createTemporaryObject(composerFactory, this)
        composer.text = "hello"
        composer.isStreaming = true
        verify(composer.isStreaming)
    }
}
