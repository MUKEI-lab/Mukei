from pathlib import Path

path = Path('.github/scripts/patch_storage_workspace_m1_backend.py')
text = path.read_text()

old = '"             | CommandType::SettingsUpdate,\\n",'
new = '"            | CommandType::SettingsUpdate,\\n",'
if text.count(old) != 1:
    raise SystemExit(f'expected one settings scope anchor, found {text.count(old)}')
text = text.replace(old, new, 1)

helper_anchor = "    p.write_text(text.replace(old, new, 1))\n\n# storage module exports\n"
helper_insert = """    p.write_text(text.replace(old, new, 1))


def replace_last(path: str, old: str, new: str) -> None:
    p = Path(path)
    text = p.read_text()
    count = text.count(old)
    if count < 1:
        raise SystemExit(f"{path}: expected at least one anchor, found {count}: {old[:100]!r}")
    position = text.rfind(old)
    p.write_text(text[:position] + new + text[position + len(old):])

# storage module exports
"""
if text.count(helper_anchor) != 1:
    raise SystemExit('replace helper anchor changed unexpectedly')
text = text.replace(helper_anchor, helper_insert, 1)

call_anchor = '''replace_once(
    "rust/crates/mukei-core/src/ui_protocol.rs",
    "                || !non_empty_bounded(&value.mime_type, 256)\\n            {\\n",
'''
call_replacement = call_anchor.replace('replace_once(', 'replace_last(', 1)
if text.count(call_anchor) != 1:
    raise SystemExit(f'expected one MIME replacement call, found {text.count(call_anchor)}')
text = text.replace(call_anchor, call_replacement, 1)

path.write_text(text)
print('normalized storage workspace M1 backend anchors')
