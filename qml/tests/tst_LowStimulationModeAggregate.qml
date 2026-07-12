import QtQuick
import QtTest
import "../theme"

TestCase {
    name: "LowStimulationModeAggregate"

    function test_reduced_motion_state_is_global_and_reversible() {
        var original = Theme.reduceMotion
        Theme.reduceMotion = true
        verify(Theme.reduceMotion)
        verify(Motion.immediateFeedback <= 120)
        verify(Motion.sheetDialog <= 300)
        Theme.reduceMotion = original
    }
}
