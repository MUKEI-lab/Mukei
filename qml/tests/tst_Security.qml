// QML Security Test Suite
// Tests for clipboard bridge, console logging absence, and security boundaries

import QtTest
import QtQuick
import "../components"

TestCase {
    name: "SecurityTests"
    
    // Test 1: Verify CopyButton uses clipboard bridge correctly
    function test_clipboard_bridge_exists() {
        // Verify mukeiClipboard is available in context
        verify(typeof mukeiClipboard !== "undefined", "mukeiClipboard bridge should be registered");
        verify(typeof mukeiClipboard.setText === "function", "mukeiClipboard.setText should be available");
    }

    function test_copy_button_sets_clipboard_text() {
        var testText = "security test content " + Math.random();
        var copyButton = Qt.createQmlObject(`
            import QtQuick
            import "../components"
            CopyButton {
                textToCopy: "${testText}"
            }
        `, testCase);

        copyButton.clicked();
        verify(mukeiClipboard.text() === testText, "CopyButton should write clipboard text");
        copyButton.destroy();
    }

    function test_accessible_properties() {
        var copyButton = Qt.createQmlObject(`
            import QtQuick
            import "../components"
            CopyButton {
                textToCopy: "accessible test"
            }
        `, testCase);
        
        verify(copyButton.Accessible.name !== "", "Accessible name should be set");
        verify(copyButton.Accessible.description !== "", "Accessible description should be set");
        
        copyButton.destroy();
    }
}
