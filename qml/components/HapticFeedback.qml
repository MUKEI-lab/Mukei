import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import "../theme"

// Platform-agnostic haptic dispatcher. On Android the bridge injects a
// C++ QVibrator wrapper into the QML context as `mukeiHaptics`; on other
// platforms `pulse()` is a silent no-op so QML sites stay identical.
// The `enabled` flag is honoured (Settings > Accessibility can suppress
// all haptics globally without touching call-sites).
QtObject {
    id: root
    property bool enabled: true

    enum Level {
        Light,
        Medium,
        Heavy
    }

    function pulse(level) {
        if (!root.enabled) return;
        // The bridge exposes `mukeiHaptics.pulse(int)` when running on
        // Android; on desktop this context property is absent, so the
        // typeof-check keeps the call a silent no-op.
        if (typeof mukeiHaptics !== "undefined" && mukeiHaptics && typeof mukeiHaptics.pulse === "function") {
            mukeiHaptics.pulse(level);
        }
    }
}
