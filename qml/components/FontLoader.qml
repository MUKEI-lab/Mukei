import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import "../theme"

// Wraps QtQuick's real FontLoader for each bundled variable font. `main.cpp`
// registers them via QFontDatabase::addApplicationFont so QML `font.family`
// bindings resolve immediately; this component exists to (a) expose a stable
// `allLoaded` signal that screens can wait on and (b) surface individual
// `status` fields for smoke-tests. Failure to load an individual file is
// non-fatal — Qt falls back to the platform default per family.
QtObject {
    id: root
    signal allLoaded
    signal loadFailed(string source, int status)

    readonly property var upright: [
        Qt.createQmlObject('import QtQuick; FontLoader { source: "qrc:/fonts/PlayfairDisplay-Variable.ttf" }', root),
        Qt.createQmlObject('import QtQuick; FontLoader { source: "qrc:/fonts/Merriweather-Variable.ttf" }', root),
        Qt.createQmlObject('import QtQuick; FontLoader { source: "qrc:/fonts/Inter-Variable.ttf" }', root),
        Qt.createQmlObject('import QtQuick; FontLoader { source: "qrc:/fonts/JetBrainsMono-Variable.ttf" }', root)
    ]
    readonly property var italic: [
        Qt.createQmlObject('import QtQuick; FontLoader { source: "qrc:/fonts/PlayfairDisplay-Italic-Variable.ttf" }', root),
        Qt.createQmlObject('import QtQuick; FontLoader { source: "qrc:/fonts/Merriweather-Italic-Variable.ttf" }', root),
        Qt.createQmlObject('import QtQuick; FontLoader { source: "qrc:/fonts/Inter-Italic-Variable.ttf" }', root),
        Qt.createQmlObject('import QtQuick; FontLoader { source: "qrc:/fonts/JetBrainsMono-Italic-Variable.ttf" }', root)
    ]

    function _checkAll() {
        for (var i = 0; i < upright.length; ++i) {
            if (upright[i].status !== FontLoader.Ready) return;
            if (italic[i].status !== FontLoader.Ready) return;
        }
        root.allLoaded();
    }

    Component.onCompleted: {
        for (var i = 0; i < upright.length; ++i) {
            upright[i].statusChanged.connect(_checkAll);
            italic[i].statusChanged.connect(_checkAll);
            if (upright[i].status === FontLoader.Error) root.loadFailed(upright[i].source, upright[i].status);
            if (italic[i].status === FontLoader.Error) root.loadFailed(italic[i].source, italic[i].status);
        }
        _checkAll();
    }
}
