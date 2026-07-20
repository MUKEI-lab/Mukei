from pathlib import Path
import runpy

runpy.run_path('.github/scripts/patch_conversation_storage_attachments.py', run_name='__main__')

path = Path('rust/crates/mukei-core/src/storage/migrations.rs')
text = path.read_text()
old = '            vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16]\n'
new = '            vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17]\n'
if text.count(old) != 1:
    raise SystemExit(f'embedded migration expectation anchor count: {text.count(old)}')
path.write_text(text.replace(old, new, 1))
print('conversation storage attachment v2 migration expectation applied')
