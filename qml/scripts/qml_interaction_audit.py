#!/usr/bin/env python3
"""Inventory interactive QML controls and detect missing test hooks or handlers.

The first rollout is report-only. Pass --strict after the baseline is cleaned to
make missing objectName/handler entries fail CI.
"""

from __future__ import annotations

import argparse
import re
import sys
from dataclasses import dataclass
from pathlib import Path

CONTROL_TYPES = {
    "Button",
    "PrimaryButton",
    "SecondaryButton",
    "GhostButton",
    "DestructiveButton",
    "IconButton",
    "ToolButton",
    "RoundButton",
    "CheckBox",
    "Switch",
    "RadioButton",
    "TabButton",
    "MenuItem",
}
HANDLER_RE = re.compile(
    r"\bon(?:Clicked|Committed|Triggered|Toggled|Activated|Accepted|Pressed|Released|CheckedChanged)\s*:"
)
OBJECT_NAME_RE = re.compile(r'\bobjectName\s*:\s*"([^"]+)"')
CONTROL_START_RE = re.compile(
    r"^(?P<indent>\s*)(?P<type>" + "|".join(sorted(CONTROL_TYPES, key=len, reverse=True)) + r")\s*\{"
)


@dataclass(frozen=True)
class Finding:
    path: str
    line: int
    control_type: str
    object_name: str
    has_handler: bool
    exempt: bool

    @property
    def status(self) -> str:
        if self.exempt:
            return "exempt"
        if not self.object_name and not self.has_handler:
            return "missing-id-and-handler"
        if not self.object_name:
            return "missing-id"
        if not self.has_handler:
            return "missing-handler"
        return "covered"


def brace_delta(line: str) -> int:
    # This scanner is intentionally lexical, not a QML parser. Braces inside quoted
    # strings are removed so normal JS/QML object blocks remain balanced.
    cleaned = re.sub(r'"(?:\\.|[^"\\])*"', '""', line)
    cleaned = cleaned.split("//", 1)[0]
    return cleaned.count("{") - cleaned.count("}")


def scan_file(path: Path, root: Path) -> list[Finding]:
    lines = path.read_text(encoding="utf-8").splitlines()
    findings: list[Finding] = []
    index = 0
    while index < len(lines):
        match = CONTROL_START_RE.match(lines[index])
        if not match:
            index += 1
            continue

        start = index
        depth = brace_delta(lines[index])
        index += 1
        while index < len(lines) and depth > 0:
            depth += brace_delta(lines[index])
            index += 1
        block = "\n".join(lines[start:index])
        name_match = OBJECT_NAME_RE.search(block)
        findings.append(
            Finding(
                path=str(path.relative_to(root)).replace("\\", "/"),
                line=start + 1,
                control_type=match.group("type"),
                object_name=name_match.group(1) if name_match else "",
                has_handler=HANDLER_RE.search(block) is not None,
                exempt="interaction-audit: exempt" in block,
            )
        )
    return findings


def markdown(findings: list[Finding]) -> str:
    lines = [
        "# QML Interaction Audit",
        "",
        "Generated from screen, shell, and composer QML sources.",
        "",
        "| File | Line | Control | objectName | Handler | Status |",
        "|---|---:|---|---|---|---|",
    ]
    for item in findings:
        lines.append(
            f"| `{item.path}` | {item.line} | `{item.control_type}` | "
            f"`{item.object_name or '—'}` | {'yes' if item.has_handler else 'no'} | "
            f"**{item.status}** |"
        )
    counts: dict[str, int] = {}
    for item in findings:
        counts[item.status] = counts.get(item.status, 0) + 1
    lines.extend(["", "## Summary", ""])
    for status in sorted(counts):
        lines.append(f"- `{status}`: {counts[status]}")
    lines.append("")
    return "\n".join(lines)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("root", type=Path, nargs="?", default=Path("qml"))
    parser.add_argument("--report", type=Path)
    parser.add_argument("--strict", action="store_true")
    args = parser.parse_args()

    root = args.root.resolve()
    targets = [root / "screens", root / "shell"]
    files = sorted(path for directory in targets for path in directory.glob("*.qml"))
    files.append(root / "components" / "ChatComposer.qml")

    findings: list[Finding] = []
    for path in files:
        if path.is_file():
            findings.extend(scan_file(path, root))

    report = markdown(findings)
    print(report)
    if args.report:
        args.report.parent.mkdir(parents=True, exist_ok=True)
        args.report.write_text(report, encoding="utf-8")

    blockers = [
        item for item in findings
        if item.status in {"missing-id", "missing-handler", "missing-id-and-handler"}
    ]
    if args.strict and blockers:
        print(f"interaction audit failed: {len(blockers)} uncovered controls", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
