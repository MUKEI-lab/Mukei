import QtQuick

// Platform-agnostic haptic dispatcher. Native haptics are an optional injected
// capability rather than an undeclared QML context dependency. Until a native
// backend is configured, pulse() is deliberately a silent no-op.
QtObject {
    id: root
    property bool enabled: true
    property var backend: null

    enum Level {
        Light,
        Medium,
        Heavy
    }

    function configure(nativeBackend) {
        backend = nativeBackend || null
    }

    function pulse(level) {
        if (!root.enabled)
            return
        if (root.backend !== null && typeof root.backend.pulse === "function")
            root.backend.pulse(level)
    }
}
