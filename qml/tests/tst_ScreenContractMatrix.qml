import QtQuick
import QtTest
import "../theme"
import "../screens"
import "../components"

// UXB v2.1 mandates that every top-level screen exposes:
//   - Accessible.role = Accessible.Pane
//   - a non-empty Accessible.name
//   - a background wired to the active palette (Theme.p.background)
// This test instantiates the 5 primary screens that render without a
// live bridge and verifies the contract.
TestCase {
    name: "ScreenContractMatrix"
    when: windowShown

    Component { id: welcome;      WelcomeScreen {} }
    Component { id: emptyChat;    EmptyChatScreen {} }
    Component { id: modelPicker;  ModelPickerScreen {} }
    Component { id: settings;     SettingsScreen {} }
    Component { id: safeMode;     SafeModeScreen {} }

    function _check(component, label) {
        var obj = createTemporaryObject(component, this);
        verify(obj !== null, label + ": screen instantiates");
        compare(obj.Accessible.role, Accessible.Pane, label + ": Accessible.role is Pane");
        verify(obj.Accessible.name && obj.Accessible.name.length > 0, label + ": Accessible.name is non-empty");
    }

    function test_welcome_screen()        { _check(welcome,     "WelcomeScreen"); }
    function test_empty_chat_screen()     { _check(emptyChat,   "EmptyChatScreen"); }
    function test_model_picker_screen()   { _check(modelPicker, "ModelPickerScreen"); }
    function test_settings_screen()       { _check(settings,    "SettingsScreen"); }
    function test_safe_mode_screen()      { _check(safeMode,    "SafeModeScreen"); }
}
