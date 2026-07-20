from pathlib import Path

path = Path('.github/scripts/patch_storage_workspace_m1_backend.py')
text = path.read_text()
old = '"             | CommandType::SettingsUpdate,\\n",'
new = '"            | CommandType::SettingsUpdate,\\n",'
if text.count(old) != 1:
    raise SystemExit(f'expected one settings scope anchor, found {text.count(old)}')
path.write_text(text.replace(old, new, 1))
print('normalized storage workspace M1 backend anchors')
