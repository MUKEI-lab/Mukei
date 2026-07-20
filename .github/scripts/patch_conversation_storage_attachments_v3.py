from pathlib import Path
import runpy

runpy.run_path('.github/scripts/patch_conversation_storage_attachments_v2.py', run_name='__main__')

path = Path('rust/crates/mukei-core/tests/universal_storage_embedded_migrations.rs')
text = path.read_text()
old = '''    assert_eq!(
        versions,
        (1..=16).collect::<Vec<_>>(),
        "embedded migrations must be contiguous through V016"
    );
'''
new = '''    assert_eq!(
        versions,
        (1..=17).collect::<Vec<_>>(),
        "embedded migrations must be contiguous through V017"
    );
'''
if text.count(old) != 1:
    raise SystemExit(f'embedded integration version anchor count: {text.count(old)}')
text = text.replace(old, new, 1)
anchor = '''    assert!(hardening
        .2
        .contains("operation_journal_terminal_evidence_immutable"));
}
'''
insert = '''    assert!(hardening
        .2
        .contains("operation_journal_terminal_evidence_immutable"));

    let conversation_attachments = migrations
        .iter()
        .find(|(version, _, _)| *version == 17)
        .expect("V017 conversation Storage attachment migration must be embedded");
    assert_eq!(
        conversation_attachments.1,
        "V017__conversation_storage_attachments"
    );
    assert!(conversation_attachments
        .2
        .contains("CREATE TABLE IF NOT EXISTS conversation_storage_attachments"));
    assert!(conversation_attachments
        .2
        .contains("conversation_storage_attachment_identity_immutable"));
}
'''
if text.count(anchor) != 1:
    raise SystemExit(f'embedded integration V017 assertion anchor count: {text.count(anchor)}')
path.write_text(text.replace(anchor, insert, 1))
print('conversation storage attachment v3 embedded migration integration contract applied')
