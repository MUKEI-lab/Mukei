# Mukei QML Frontend (Qt 6.5+)

Editorial-luxury local-first UI for the Mukei kernel. The production path consumes
`mukei-bridge` through CXX-Qt. For standalone desktop UI iteration, `main.cpp`
also provides an explicit compatibility implementation; it must not be confused
with proof that the Rust production bridge is active.

## Layout

```text
qml/
├── CMakeLists.txt          Qt 6 executable + qt_add_qml_module target
├── main.cpp                Application entry + standalone compatibility bridge
├── qml.qrc                 Explicit resource manifest
├── MainWindow.qml          Root ApplicationWindow
├── architecture/           Contract/capability architecture helpers
├── events/                 Raw event acceptance and dispatch
├── stores/                 Scoped reactive projections
├── shell/                  Application shell and lifecycle routing
├── theme/                  Theme, type, spacing, and motion singletons
├── components/             Reusable QML components
├── screens/                Top-level pages and full-screen flows
├── tests/                  23 behavioural Qt Quick Test files
└── assets/
    ├── fonts/              8 committed font assets
    └── icons/              27 committed SVG icons
```

## Runtime contract

The QML layer is projection-oriented:

- screens emit intents through `IntentDispatcher`;
- `OperationStore` owns command/operation lifecycle projection;
- `EventDispatcher` is the only raw event parser;
- scoped stores own chat, recovery, model, document, settings, diagnostics,
  storage, and operation projections;
- contract negotiation decides whether the active peer supports Protocol V2
  or only the isolated legacy compatibility event mode.

Production Protocol V2 provides command acknowledgements, event identity,
per-stream sequencing, bounded idempotent replay protection, and correlated
operation lifecycle events. See
[`../docs/PROTOCOL_V2_ARCHITECTURE.md`](../docs/PROTOCOL_V2_ARCHITECTURE.md).

The standalone desktop compatibility implementation may acknowledge V2 commands
while advertising legacy event delivery. That mode is intentionally lower
assurance and is not presented as equivalent to the production Rust bridge.

## Build: desktop smoke

```bash
# Requires Qt 6.5+, CMake, and Ninja.
cmake -S qml -B /tmp/mukei-qml-build -G Ninja
cmake --build /tmp/mukei-qml-build
ctest --test-dir /tmp/mukei-qml-build --output-on-failure
```

Run the standalone shell from the generated build directory after a successful
build.

## Linking the Rust bridge

Build the bridge first:

```bash
cd rust
cargo build -p mukei-bridge --release
```

Android production-oriented builds are expected to use SQLCipher and an explicit
runtime deployment mode. A representative build shape is:

```bash
cd rust
cargo build   -p mukei-bridge   --profile android-release   --target aarch64-linux-android   --no-default-features   --features "android_keystore,network,sqlcipher,runtime_production,runtime_hardening"
```

Exact Android packaging still depends on the Qt/NDK/Gradle integration and is a
release gate.

## Important bridge-facing surfaces

Depending on the active build/host integration, QML expects bridge-owned objects
for agent/bridge operations and SAF/document access, plus optional platform
helpers such as clipboard and haptics.

Do not infer production capability from object presence alone. Capability and
protocol negotiation are the source of truth.

## Test inventory

The current source contains 23 `tst_*.qml` behavioural tests covering areas such
as Protocol V2, event dispatch, operation snapshots, contract negotiation,
accessibility, RTL, font scaling, tab order, destructive confirmation, and
low-stimulation behavior.

Presence of these files is not a pass claim. Run the Qt 6.5+ QuickTest/CTest
matrix for the current snapshot.

## Assets provenance

- Fonts are committed under `assets/fonts/`; preserve their upstream license
  notices and provenance.
- Icons are committed under `assets/icons/`; preserve their upstream license
  notices and provenance.

Replace asset sets deliberately rather than silently editing third-party
artifacts in place.
