pragma Singleton
import QtQuick
import "../architecture"

QtObject {
    readonly property var allowedRoutes: [
        "boot", "unlock", "welcome", "recovery", "chat", "models", "downloads",
        "documents", "settings", "security", "diagnostics", "compatibility"
    ]
    property string currentRoute: "boot"
    property var currentParameters: ({})
    property var history: []
    property bool lifecycleLocked: true

    signal routeChanged(string route, var parameters)
    signal navigationRejected(string route, string reason)

    function isAllowed(route) {
        return allowedRoutes.indexOf(route) >= 0
    }

    function navigate(route, parameters, replace) {
        if (!isAllowed(route)) {
            navigationRejected(route, "unknown_route")
            return false
        }
        if (lifecycleLocked && ["boot", "unlock", "welcome", "security", "compatibility"].indexOf(route) < 0) {
            navigationRejected(route, "lifecycle_locked")
            return false
        }
        if (!replace && currentRoute !== route) {
            var nextHistory = history.slice(0)
            nextHistory.push({ route: currentRoute, parameters: currentParameters })
            if (nextHistory.length > 16)
                nextHistory.shift()
            history = nextHistory
        }
        currentRoute = route
        currentParameters = parameters || ({})
        UiSessionStore.setActiveRoute(route, currentParameters)
        routeChanged(route, currentParameters)
        return true
    }

    function goBack() {
        if (history.length === 0)
            return false
        var nextHistory = history.slice(0)
        var previous = nextHistory.pop()
        history = nextHistory
        currentRoute = previous.route
        currentParameters = previous.parameters || ({})
        UiSessionStore.setActiveRoute(currentRoute, currentParameters)
        routeChanged(currentRoute, currentParameters)
        return true
    }

    function syncWithLifecycle(lifecycleState) {
        var route = PresentationPolicy.routeForLifecycle(lifecycleState)
        lifecycleLocked = lifecycleState !== "ready" && lifecycleState !== "degraded"
        if (lifecycleLocked || ["boot", "unlock", "welcome", "security", "compatibility"].indexOf(currentRoute) >= 0)
            navigate(route, ({}), true)
    }
}
