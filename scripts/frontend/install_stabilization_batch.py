#!/usr/bin/env python3
from __future__ import annotations

import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
REQUIRED_FILES = [
    "qml/tests/tst_InteractionContracts.qml",
    "qml/tests/tst_StartupLifecycle.qml",
    "docs/FRONTEND_STABILIZATION_STRATEGY.md",
    "reports/frontend/INTERACTION_MATRIX.md",
]


def replace_once(path: Path, old: str, new: str, marker: str) -> None:
    text = path.read_text(encoding="utf-8")
    if marker in text:
        return
    if old not in text:
        raise SystemExit(f"missing interaction anchor in {path}: {marker}")
    path.write_text(text.replace(old, new, 1), encoding="utf-8")


def main() -> None:
    for relative in REQUIRED_FILES:
        if not (ROOT / relative).is_file():
            raise SystemExit(f"missing stabilization contract: {relative}")

    composer = ROOT / "qml/components/ChatComposer.qml"
    replace_once(composer, "FocusScope {\n    id: root\n", "FocusScope {\n    id: root\n    objectName: \"chatComposer\"\n", "objectName: \"chatComposer\"")
    replace_once(composer, "        IconButton {\n            iconSource: \"qrc:/icons/attach.svg\"", "        IconButton {\n            objectName: \"chatAttachButton\"\n            iconSource: \"qrc:/icons/attach.svg\"", "objectName: \"chatAttachButton\"")
    replace_once(composer, "        TextArea {\n            id: textArea\n", "        TextArea {\n            id: textArea\n            objectName: \"chatMessageEditor\"\n", "objectName: \"chatMessageEditor\"")
    replace_once(composer, "        Button {\n            id: sendButton\n", "        Button {\n            id: sendButton\n            objectName: \"chatSendButton\"\n", "objectName: \"chatSendButton\"")

    staged = [*REQUIRED_FILES, "qml/components/ChatComposer.qml"]
    subprocess.run(["git", "add", *staged], cwd=ROOT, check=True)
    print("frontend stabilization contracts verified")


if __name__ == "__main__":
    main()
