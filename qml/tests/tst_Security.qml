// QML Security Test Suite
// Tests for clipboard bridge, console logging absence, and security boundaries

import QtTest
import QtQuick
import "../components"

TestCase {
    id: testCase
    name: "SecurityTests"
    
    // Test 1: Verify CopyButton uses clipboard bridge correctly
    function test_clipboard_bridge_exists() {
        // Verify mukeiClipboard is available in context
        verify(typeof mukeiClipboard !== "undefined", "mukeiClipboard bridge should be registered");
        verify(typeof mukeiClipboard.setText === "function", "mukeiClipboard.setText should be available");
    }
    
    // Test 2: Verify CopyButton invokes clipboard without console logging
    function test_copyButton_no_console_logging() {
        var copyButton = Qt.createQmlObject(`
            import QtQuick
            import "../components"
            CopyButton {
                textToCopy: "test content"
            }
        `, testCase);
        
        // Spy on console methods (should not be called)
        var consoleWarnCalled = false;
        var originalWarn = console.warn;
        console.warn = function() { consoleWarnCalled = true; };
        
        // Trigger click
        copyButton.clicked();
        
        // Restore console
        console.warn = originalWarn;
        
        // Verify no console warning was logged
        verify(!consoleWarnCalled, "CopyButton should not log to console when clipboard is available");
        
        copyButton.destroy();
    }
    
    // Test 3: Verify clipboard receives correct text
    function test_clipboard_receives_text() {
        var testText = "security test content " + Math.random();
        var copyButton = Qt.createQmlObject(`
            import QtQuick
            import "../components"
            CopyButton {
                textToCopy: "${testText}"
            }
        `, testCase);
        
        copyButton.clicked();
        compare(mukeiClipboard.text(), testText, "CopyButton should write exact text to clipboard bridge");
        
        copyButton.destroy();
    }
    
    // Test 6: Verify accessible properties are set (security through accessibility)
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
