pragma Singleton
import QtQuick

QtObject {
    enum Mode { Compact, Medium, Expanded }

    property real viewportWidth: 0
    property real viewportHeight: 0

    readonly property int mode: viewportWidth < 600
                                ? ResponsiveStore.Mode.Compact
                                : viewportWidth < 840
                                  ? ResponsiveStore.Mode.Medium
                                  : ResponsiveStore.Mode.Expanded
    readonly property bool compact: mode === ResponsiveStore.Mode.Compact
    readonly property bool medium: mode === ResponsiveStore.Mode.Medium
    readonly property bool expanded: mode === ResponsiveStore.Mode.Expanded
    readonly property real edgePadding: compact ? Spacing.lg : medium ? Spacing.xl : Spacing.xxl
    readonly property real contentMaxWidth: expanded ? 960 : medium ? 760 : Math.max(0, viewportWidth)

    function updateViewport(width, height) {
        viewportWidth = Math.max(0, width || 0)
        viewportHeight = Math.max(0, height || 0)
    }
}
