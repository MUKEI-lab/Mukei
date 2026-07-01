# Mukei QML Frontend (Qt 6.5+)

Editorial-luxury on-device chat shell for the Mukei kernel. Consumes the
`mukei-bridge` (CXX-Qt) FFI or, when running standalone for iterative UI
work, the C++ stubs bundled in `main.cpp`.

## Layout

```
qml/
├── CMakeLists.txt          Qt6 executable + qt_add_qml_module target
├── main.cpp                Application entry + agent/bridge/SAF stubs
├── qml.qrc                 Explicit resource manifest (fonts, icons, code)
├── MainWindow.qml          Root ApplicationWindow
├── theme/                  Theme, Type, Spacing, Motion singletons
├── components/             34 reusable QML types (buttons, bubbles, sheets, …)
├── screens/                10 top-level Pages / FullScreenModals
├── tests/                  QtTest stubs (auto-run under `qmltestrunner`)
└── assets/
    ├── fonts/              8 variable-axis SIL OFL fonts (see fonts/README.md)
    └── icons/              27 semantically-unique Phosphor v2.0.8 SVGs (MIT)
```

## Build (desktop smoke)

```bash
# 1. Install Qt 6.5 or newer (system package manager or online installer).
#    Fedora:   sudo dnf install qt6-qtdeclarative-devel qt6-qtsvg-devel cmake ninja-build
#    Debian:   sudo apt install qt6-declarative-dev qt6-svg-dev cmake ninja-build
#    macOS:    brew install qt cmake ninja

# 2. Configure + build.
cmake -S qml -B qml/build -G Ninja
cmake --build qml/build

# 3. Run the shell (standalone stubs, no bridge required).
./qml/build/mukei
```

## Linking to the Rust bridge

The CMake file probes `../rust/target/{host-triple}/release/libmukei_bridge.{so,dylib}`.
Build it first:

```bash
cd rust
cargo build -p mukei-bridge --release
```

If the file is present, `target_link_libraries(mukei PRIVATE …)` picks it
up automatically; if not, the app still launches with the stubs in
`main.cpp` so pure-QML iteration is never blocked.

## Runtime contract

| Context property | Type                | Provided by      |
| ---------------- | ------------------- | ---------------- |
| `mukeiAgent`     | `MukeiAgent`        | `mukei-bridge`   |
| `mukeiBridge`    | `MukeiBridge`       | `mukei-bridge`   |
| `safRegistry`    | `SafRegistry`       | `mukei-bridge`   |
| `mukeiClipboard` | `QClipboard` shim   | `mukei-bridge` (optional; CopyButton falls back to warning) |
| `mukeiHaptics`   | `QVibrator` shim    | `mukei-bridge` (Android only; desktop no-op) |

## Assets provenance

- **Fonts** — Playfair Display, Merriweather, Inter, JetBrains Mono (SIL
  OFL 1.1) fetched from https://github.com/google/fonts. Variable-axis
  TTFs. See `assets/fonts/README.md`.
- **Icons** — Phosphor Icons v2.0.8 (MIT) fetched from
  https://github.com/phosphor-icons/core. 27 semantically distinct
  `regular` weight glyphs, viewBox 0 0 256 256, `fill="currentColor"` so
  they respect the QML palette at render time.

Both asset sets are committed verbatim; do not modify individual files
in-place — replace the whole set from upstream if you need to refresh.
