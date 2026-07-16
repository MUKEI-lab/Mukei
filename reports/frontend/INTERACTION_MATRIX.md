# Frontend Interaction Matrix

Status values: `covered`, `needs-test`, `backend-blocked`, `not-implemented`.

| Surface | Control | Type | Dependency | Expected result | Status |
|---|---|---|---|---|---|
| Safe mode | Continue Anyway | local | lifecycle store | open chat in limited mode | covered |
| Safe mode | View Crash Log | local | navigation store | open diagnostics while lifecycle is locked | covered |
| Safe mode | Reset All Data | backend | reset contract | preserve model and reset private state | not-implemented |
| Diagnostics | Refresh | hybrid | snapshots/bridge | refresh privacy-safe runtime rows | needs-test |
| Diagnostics | Create safe report | backend | diagnostics export | create local redacted report | backend-blocked |
| Diagnostics | Back | local | navigation history | return to previous route | needs-test |
| Chat | Open conversations | local | drawer | open mobile drawer | needs-test |
| Chat | Settings | local | navigation store | open settings | needs-test |
| Chat | Models | local | navigation store | open model manager | needs-test |
| Composer | Attach | backend | document grant | open local document picker | backend-blocked |
| Composer | Send | backend | ready model | submit correlated chat command | backend-blocked |
| Models | Download | backend | ready runtime | start durable download | backend-blocked |
| Models | Select | backend | installed model | activate verified model | backend-blocked |

## Rule

A visible control cannot be marked release-ready unless it has a stable object ID, an explicit handler, and an automated expected-result assertion.
