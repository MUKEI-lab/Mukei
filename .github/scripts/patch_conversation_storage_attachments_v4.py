from pathlib import Path
import runpy

runpy.run_path('.github/scripts/patch_conversation_storage_attachments_v3.py', run_name='__main__')

path = Path('rust/crates/mukei-core/tests/universal_storage_v016_upgrade.rs')
text = path.read_text()
replacements = [
    (
        '//! through the normal migration engine by applying V016 only.\n',
        '//! through the normal migration engine by applying V016 and later append-only migrations.\n',
    ),
    ('    assert_eq!(bundled.last().map(|entry| entry.0), Some(16));\n',
     '    assert_eq!(bundled.last().map(|entry| entry.0), Some(17));\n'),
    ('        .expect("upgrade V015 database through embedded V016");\n'
     '    assert_eq!(upgraded.len(), 1, "only V016 should be pending");\n'
     '    assert_eq!(upgraded[0].id, 16);\n'
     '    assert_eq!(\n'
     '        upgraded[0].name,\n'
     '        "V016__storage_identity_and_recovery_hardening"\n'
     '    );\n',
     '        .expect("upgrade V015 database through embedded V017");\n'
     '    assert_eq!(upgraded.len(), 2, "V016 and V017 should be pending");\n'
     '    assert_eq!(upgraded[0].id, 16);\n'
     '    assert_eq!(\n'
     '        upgraded[0].name,\n'
     '        "V016__storage_identity_and_recovery_hardening"\n'
     '    );\n'
     '    assert_eq!(upgraded[1].id, 17);\n'
     '    assert_eq!(upgraded[1].name, "V017__conversation_storage_attachments");\n'),
    ('        assert_eq!(max_version, 16);\n', '        assert_eq!(max_version, 17);\n'),
    ('        assert_eq!(user_version, 16);\n', '        assert_eq!(user_version, 17);\n'),
    ('        .expect("repeated boot after V016");\n', '        .expect("repeated boot after V017");\n'),
    ('        "V016 must be idempotent at migration-engine level"\n',
     '        "V016/V017 upgrade must be idempotent at migration-engine level"\n'),
]
for old, new in replacements:
    count = text.count(old)
    if count != 1:
        raise SystemExit(f'V016 upgrade anchor count {count}: {old[:100]!r}')
    text = text.replace(old, new, 1)

anchor = '''        let hardening_trigger_count: i64 = connection.query_row(
            "SELECT COUNT(*) FROM sqlite_master
             WHERE type = 'trigger' AND name = 'storage_node_identity_immutable'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(hardening_trigger_count, 1);

        let user_version: i64 =
'''
insert = '''        let hardening_trigger_count: i64 = connection.query_row(
            "SELECT COUNT(*) FROM sqlite_master
             WHERE type = 'trigger' AND name = 'storage_node_identity_immutable'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(hardening_trigger_count, 1);

        let attachment_table_count: i64 = connection.query_row(
            "SELECT COUNT(*) FROM sqlite_master
             WHERE type = 'table' AND name = 'conversation_storage_attachments'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(attachment_table_count, 1);

        let user_version: i64 =
'''
if text.count(anchor) != 1:
    raise SystemExit(f'V017 upgrade table assertion anchor count: {text.count(anchor)}')
path.write_text(text.replace(anchor, insert, 1))
print('conversation storage attachment v4 V015-to-V017 upgrade contract applied')
