pragma Singleton
import QtQuick

QtObject {
    id: root
    property var agentSource: null
    property string themeMode: "dolce_vita"
    property bool reduceMotion: false
    property bool highContrast: false
    property int fontScalePercent: 100
    property int temperatureMilli: 700
    property int maxTokens: 1024
    property int topPMilli: 950
    property string remotePolicy: "local_only"
    property bool hydrated: false
    property bool loading: false
    property string lastRequestId: ""

    signal preferenceUpdated(string key)
    signal hydrationCompleted

    function configure(agent) { agentSource = agent }

    function parseStored(row) {
        try { return JSON.parse(row.value_json) } catch (error) { return null }
    }

    function applyTheme() {
        Theme.mode = themeMode === "espresso" ? Theme.Mode.Espresso
                   : themeMode === "taupe" ? Theme.Mode.Taupe
                                            : Theme.Mode.DolceVita
        Theme.reduceMotion = reduceMotion
        Theme.highContrast = highContrast
        Theme.scale = Math.max(0.85, Math.min(2.0, fontScalePercent / 100))
    }

    function applyRows(rows) {
        if (Array.isArray(rows)) {
            for (var i = 0; i < rows.length; ++i) {
                var row = rows[i]
                var value = parseStored(row)
                switch (row.key) {
                case "theme_mode": themeMode = String(value); break
                case "reduce_motion": reduceMotion = value === true; break
                case "high_contrast": highContrast = value === true; break
                case "font_scale_percent": fontScalePercent = Number(value || 100); break
                case "temperature_milli": temperatureMilli = Number(value || 700); break
                case "max_tokens_default": maxTokens = Number(value || 1024); break
                case "top_p_milli": topPMilli = Number(value || 950); break
                case "remote_feature_policy": remotePolicy = String(value || "local_only"); break
                }
            }
        }
        applyTheme()
        loading = false
        hydrated = true
        hydrationCompleted()
    }

    function hydrate() {
        if (loading)
            return
        hydrated = false
        if (agentSource === null || typeof agentSource.settings_snapshot_json !== "function") {
            applyRows([])
            return
        }
        loading = true
        try {
            var rows = JSON.parse(agentSource.settings_snapshot_json())
            if (rows && rows.accepted === true) {
                lastRequestId = rows.request_id || ""
                return
            }
            if (rows && rows.error) {
                loading = false
                hydrated = true
                ErrorStore.push(rows.error, "ERR_UI_SETTINGS_SNAPSHOT")
                hydrationCompleted()
            } else {
                applyRows(rows)
            }
        } catch (error) {
            loading = false
            hydrated = true
            ErrorStore.push({ code: "ERR_UI_SETTINGS_SNAPSHOT", severity: "warning", recoverable: true,
                              safe_message: qsTr("Preferences could not be restored.") })
            hydrationCompleted()
        }
    }

    function update(key, value) {
        switch (key) {
        case "theme_mode": themeMode = String(value); break
        case "reduce_motion": reduceMotion = value === true; break
        case "high_contrast": highContrast = value === true; break
        case "font_scale_percent": fontScalePercent = Number(value); break
        case "temperature_milli": temperatureMilli = Number(value); break
        case "max_tokens_default": maxTokens = Number(value); break
        case "top_p_milli": topPMilli = Number(value); break
        case "remote_feature_policy": remotePolicy = String(value); break
        default: break
        }
        applyTheme()
        if (agentSource !== null && typeof agentSource.update_setting === "function")
            agentSource.update_setting(key, value)
        preferenceUpdated(key)
    }

    Connections {
        target: root.agentSource
        ignoreUnknownSignals: true
        function onAsync_result(resultJson) {
            var result
            try { result = JSON.parse(resultJson) } catch (error) { return }
            if (!result || result.domain !== "settings.snapshot"
                    || result.request_id !== root.lastRequestId || result.current === false)
                return
            if (result.ok === true)
                root.applyRows(result.payload)
            else {
                root.loading = false
                root.hydrated = true
                ErrorStore.push(result.error, "ERR_UI_SETTINGS_SNAPSHOT")
                root.hydrationCompleted()
            }
        }
    }
}
