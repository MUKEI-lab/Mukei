from __future__ import annotations

import hashlib
import sys
from pathlib import Path

EXPECTED_OUTPUT_SHA256 = "7e568026c642ae97634802610a8fdd1ab39fee1cacaac2e477ca710a51f66cb1"

REPLACEMENTS = [
    ('\'llama_cpp = ["mukei-core/llama_cpp"]\\n\',', '\'llama_cpp          = ["mukei-core/llama_cpp"]\\n\','),
    ('\'llama_cpp = []\\n\',', '\'llama_cpp          = []\\n\','),
    ('\'runtime_production = []\\n\',', '\'runtime_production  = []\\n\','),
    ('\'runtime_production = ["llama_cpp"]\\n\',', '\'runtime_production  = ["llama_cpp"]\\n\','),
    ("'''                visible: ModelStore.restartRequired || ModelStore.sessionMessage.length > 0\n'''", "'''            visible: ModelStore.restartRequired || ModelStore.sessionMessage.length > 0\n'''") ,
    ("'''                visible: ModelStore.activationInProgress || ModelStore.activationFailed\n                         || ModelStore.restartRequired || ModelStore.sessionMessage.length > 0\n'''", "'''            visible: ModelStore.activationInProgress || ModelStore.activationFailed\n                     || ModelStore.restartRequired || ModelStore.sessionMessage.length > 0\n'''") ,
    ("'''                       : qsTr(\"The selected model will be used after a supported engine session starts.\")\n'''", "'''                      : qsTr(\"The selected model will be used after a supported engine session starts.\")\n'''") ,
    ("'''                       : ModelStore.activationInProgress\n                         ? qsTr(\"The selected model is being verified and activated.\")\n                         : ModelStore.activationFailed\n                           ? qsTr(\"The replacement model could not be activated; the previous ready model remains active when available.\")\n                           : qsTr(\"No model backend is active yet.\")\n'''", "'''                      : ModelStore.activationInProgress\n                        ? qsTr(\"The selected model is being verified and activated.\")\n                        : ModelStore.activationFailed\n                          ? qsTr(\"The replacement model could not be activated; the previous ready model remains active when available.\")\n                          : qsTr(\"No model backend is active yet.\")\n'''") ,
]


def main() -> None:
    if len(sys.argv) != 2:
        raise SystemExit("usage: native-inference-patch.corrections.py <patch-script>")
    path = Path(sys.argv[1])
    content = path.read_text()
    for old, new in REPLACEMENTS:
        count = content.count(old)
        if count != 1:
            raise RuntimeError(f"expected exactly one correction anchor, found {count}: {old!r}")
        content = content.replace(old, new, 1)
    path.write_text(content)
    digest = hashlib.sha256(path.read_bytes()).hexdigest()
    if digest != EXPECTED_OUTPUT_SHA256:
        raise RuntimeError(f"corrected patch SHA mismatch: expected {EXPECTED_OUTPUT_SHA256}, got {digest}")
    print(f"corrected patch SHA-256: {digest}")


if __name__ == "__main__":
    main()
