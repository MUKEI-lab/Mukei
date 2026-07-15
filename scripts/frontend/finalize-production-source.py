#!/usr/bin/env python3
"""Finalize canonical QML/C++ integration source after the UXB migration.

This script is deliberately narrow and idempotent. It only repairs the two
integration seams proven by CI: duplicate stub initialization completions and
consistent camelCase/snake_case event signals for both stub bridge objects.
"""
from __future__ import annotations

from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
MAIN_CPP = ROOT / "qml/main.cpp"

STUB_COMPLETION = '''                emitOperationLifecycle(context, true, QJsonObject{
                    {QStringLiteral("initialized"), true},
                    {QStringLiteral("runtime"), QStringLiteral("stub")}
                });'''


def collapse_stub_completions(text: str) -> str:
    while text.count(STUB_COMPLETION) > 1:
        text = text.replace(STUB_COMPLETION + "\n" + STUB_COMPLETION,
                            STUB_COMPLETION, 1)
    if text.count(STUB_COMPLETION) != 1:
        raise SystemExit(
            f"expected exactly one stub initialization completion; found {text.count(STUB_COMPLETION)}"
        )
    return text


def ensure_bridge_camel_signal(text: str) -> str:
    class_start = text.find("class MukeiBridgeStub final")
    class_end = text.find("class SafRegistryStub final", class_start)
    if class_start < 0 or class_end < 0:
        raise SystemExit("MukeiBridgeStub class boundaries not found")

    block = text[class_start:class_end]
    snake = "    void event_emitted(const QString &eventJson);"
    camel = "    void eventEmitted(const QString &eventJson);"
    if snake not in block:
        raise SystemExit("MukeiBridgeStub snake_case event signal missing")
    if camel not in block:
        block = block.replace(snake, snake + "\n" + camel, 1)
        text = text[:class_start] + block + text[class_end:]
    return text


def validate(text: str) -> None:
    bridge_start = text.find("class MukeiBridgeStub final")
    bridge_end = text.find("class SafRegistryStub final", bridge_start)
    bridge = text[bridge_start:bridge_end]
    required = [
        "void event_emitted(const QString &eventJson);",
        "void eventEmitted(const QString &eventJson);",
        "emit eventEmitted(eventJson);",
    ]
    for marker in required:
        if marker not in bridge:
            raise SystemExit(f"bridge event contract marker missing: {marker}")
    if text.count(STUB_COMPLETION) != 1:
        raise SystemExit("stub initialization completion is not unique")


def main() -> int:
    text = MAIN_CPP.read_text(encoding="utf-8")
    text = collapse_stub_completions(text)
    text = ensure_bridge_camel_signal(text)
    validate(text)
    MAIN_CPP.write_text(text, encoding="utf-8")
    print("Canonical production source finalized")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
