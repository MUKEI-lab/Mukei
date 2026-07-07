// QML Security Test Suite
// Tests for clipboard bridge exposure and static CopyButton security invariants.

import QtTest
import QtQuick

TestCase {
    id: testCase
    name: "SecurityTests"

    function test_clipboard_bridge_exists() {
        verify(typeof mukeiClipboard !== "undefined", "mukeiClipboard bridge should be registered")
        verify(typeof mukeiClipboard.setText === "function", "mukeiClipboard.setText should be available")
        verify(typeof mukeiClipboard.text !== "undefined", "mukeiClipboard.text property should be available")
    }

    function test_clipboard_round_trip() {
        var testText = "security test content 2026-07-04"
        mukeiClipboard.setText(testText)
        compare(mukeiClipboard.text, testText, "clipboard bridge should preserve exact text")
    }

    function test_copyButton_uses_clipboard_without_console_logging() {
        var source = securityInspector.readFile("components/CopyButton.qml")
        verify(source.length > 0, "CopyButton.qml should be readable from the test harness")
        verify(source.indexOf("mukeiClipboard.setText") !== -1, "CopyButton should delegate writes to the clipboard bridge")
        verify(source.indexOf("console.") === -1, "CopyButton should not log to console during clipboard actions")
    }

    function test_accessible_properties_declared() {
        var source = securityInspector.readFile("components/CopyButton.qml")
        verify(source.indexOf("Accessible.name") !== -1, "CopyButton should declare an accessible name")
        verify(source.indexOf("Accessible.description") !== -1, "CopyButton should declare an accessible description")
    }
}
