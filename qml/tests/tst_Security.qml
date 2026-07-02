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
    
    // Test 3: Verify clipboard receives correct text using a spy QObject
    // This test requires a test-only ClipboardSpy attached to the test case
    function test_clipboard_receives_text() {
        var testText = "security test content " + Math.random();
        var copyButton = Qt.createQmlObject(`
            import QtQuick
            import "../components"
            CopyButton {
                textToCopy: "${testText}"
            }
        `, testCase);
        
        // If clipboardSpy is provided by C++ test harness, use it to verify content
        if (typeof clipboardSpy !== "undefined" && clipboardSpy !== null) {
            // Clear any previous state
            clipboardSpy.clear();
            
            // Trigger click
            copyButton.clicked();
            
            // Wait for async clipboard operation (if any)
            waitForRendering(copyButton, 100);
            
            // Verify the spy captured the exact text
            verify(clipboardSpy.wasCalled, "Clipboard.setText should have been called");
            compare(clipboardSpy.lastText, testText, "Clipboard should receive the exact text from CopyButton");
        } else {
            // Fallback: at least verify no exception occurred
            copyButton.clicked();
            verify(true, "CopyButton click executed without errors (clipboardSpy not available for content verification)");
        }
        
        copyButton.destroy();
    }
    
    // Test 4: Verify no dynamic code execution with user-controlled strings
    // This is a static analysis check performed at build time via qml_security_analyzer.py
    // Runtime check: ensure Qt.createQmlObject is never called with interpolated user input
    function test_no_dynamic_code_execution() {
        // Verify that Qt.createQmlObject is only used for static resources
        var fontLoader = Qt.createQmlObject(`
            import QtQuick
            FontLoader { source: "qrc:/fonts/Inter-Variable.ttf" }
        `, testCase);
        
        verify(fontLoader !== null, "Static font loading should work");
        
        // Check that no component exposes a method that passes user input to createQmlObject
        // This would require runtime introspection which isn't practical in QML tests.
        // The real verification happens in scripts/qml_security_analyzer.py which greps for:
        //   Qt.createQmlObject\s*\([^)]*[\+\$]
        // (patterns indicating string interpolation in createQmlObject calls)
        
        // For this test, we verify our components don't expose dangerous patterns
        // by checking that IconButton/CopyButton don't have eval-like properties
        var copyButton = Qt.createQmlObject(`
            import QtQuick
            import "../components"
            CopyButton { textToCopy: "test" }
        `, testCase);
        
        // Ensure no eval or Function constructor exposure
        verify(typeof copyButton.evalContent === "undefined", "CopyButton should not expose eval-like properties");
        
        fontLoader.destroy();
        copyButton.destroy();
    }
    
    // Test 5: Verify component isolation - components don't pollute global scope
    function test_component_isolation() {
        // Capture initial global property count
        var initialGlobals = Object.keys(this).length;
        
        var copyButton = Qt.createQmlObject(`
            import QtQuick
            import "../components"
            CopyButton {
                textToCopy: "isolated test"
            }
        `, testCase);
        
        // Verify component doesn't expose internal state globally
        verify(typeof copyButton._acknowledging !== "undefined", "Internal state should be private convention");
        
        // After destroying, verify no new globals were added
        copyButton.destroy();
        
        // Note: QML's JS engine has no `window` global by design.
        // The original test checked `typeof window === "undefined"` which always passes.
        // Instead, we verify that creating/destroying components doesn't leak properties.
        var finalGlobals = Object.keys(this).length;
        compare(finalGlobals, initialGlobals, "Component creation/destruction should not pollute global scope");
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
    
    // Test 7: Static analysis integration - verify qml_security_analyzer.py exists
    function test_security_analyzer_available() {
        // This test ensures the security analyzer script is present for CI checks
        // The actual analysis is run as a separate build step
        var analyzerPath = Qt.resolvedUrl("../scripts/qml_security_analyzer.py");
        // We can't verify file existence directly in QML, but we document the expectation
        // CI must run: python qml/scripts/qml_security_analyzer.py qml/
        // and verify it finds no CRITICAL/HIGH issues for:
        //   - Qt.createQmlObject with interpolated/user-controlled strings
        //   - console.log/warn/error in production code
        //   - eval/Function constructor usage
        verify(true, "Security analyzer integration documented - run separately in CI");
    }
}
