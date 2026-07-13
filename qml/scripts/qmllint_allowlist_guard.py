#!/usr/bin/env python3
"""Fail CI on any unreviewed qmllint unqualified-access diagnostic.

Qt 6.5 cannot statically model a few deliberate runtime/injection boundaries in
this codebase. qmllint therefore emits those exact expressions as
``unqualified`` diagnostics. The category is kept informational in
``qml/.qmllint.ini`` only so this guard can inspect every occurrence and enforce
an exact-source allowlist. Any new expression, file, or stale allowlist entry is
an error.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

HEADER_RE = re.compile(
    r"^(?:Info|Warning): (?P<path>.*?):\d+:\d+: "
    r"Unqualified access \[unqualified\]$"
)

ALLOWED: set[tuple[str, str]] = {
    (
        "events/EventDispatcher.qml",
        'property var agentSource: typeof mukeiAgent !== "undefined" ? mukeiAgent : null',
    ),
    (
        "events/EventDispatcher.qml",
        'property var bridgeSource: typeof mukeiBridge !== "undefined" ? mukeiBridge : null',
    ),
    (
        "architecture/IntentDispatcher.qml",
        'property var contractStoreRef: typeof ContractStore !== "undefined" ? ContractStore : null',
    ),
    (
        "architecture/IntentDispatcher.qml",
        'property var capabilityStoreRef: typeof CapabilityStore !== "undefined" ? CapabilityStore : null',
    ),
    (
        "architecture/IntentDispatcher.qml",
        'property var chatStoreRef: typeof ChatStore !== "undefined" ? ChatStore : null',
    ),
    (
        "architecture/IntentDispatcher.qml",
        'property var operationStoreRef: typeof OperationStore !== "undefined" ? OperationStore : null',
    ),
    (
        "architecture/IntentDispatcher.qml",
        "&& ErrorStore !== null",
    ),
    (
        "stores/ChatStore.qml",
        "? mukeiTimelineModel : fallbackTimelineModel",
    ),
}


def qml_relative_path(raw_path: str) -> str:
    normalized = raw_path.replace("\\", "/")
    marker = "/qml/"
    if marker not in normalized:
        return normalized
    return normalized.split(marker, 1)[1]


def main() -> int:
    if len(sys.argv) != 2:
        print("usage: qmllint_allowlist_guard.py <qmllint-log>", file=sys.stderr)
        return 2

    log_path = Path(sys.argv[1])
    if not log_path.is_file():
        print(f"qmllint log does not exist: {log_path}", file=sys.stderr)
        return 2

    lines = log_path.read_text(encoding="utf-8", errors="replace").splitlines()
    seen: set[tuple[str, str]] = set()
    violations: list[tuple[str, str]] = []

    for index, line in enumerate(lines):
        match = HEADER_RE.match(line)
        if match is None:
            continue

        source = lines[index + 1].strip() if index + 1 < len(lines) else ""
        key = (qml_relative_path(match.group("path")), source)
        if key in ALLOWED:
            seen.add(key)
        else:
            violations.append(key)

    stale = sorted(ALLOWED - seen)

    if violations:
        print("Unapproved qmllint unqualified-access diagnostics:", file=sys.stderr)
        for path, source in violations:
            print(f"  {path}: {source}", file=sys.stderr)

    if stale:
        print("Stale qmllint allowlist entries must be removed or updated:", file=sys.stderr)
        for path, source in stale:
            print(f"  {path}: {source}", file=sys.stderr)

    if violations or stale:
        return 1

    print(f"qmllint unqualified-access policy passed ({len(seen)} reviewed exceptions).")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
