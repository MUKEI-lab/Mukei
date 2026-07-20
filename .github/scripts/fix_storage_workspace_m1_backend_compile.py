from pathlib import Path


def replace_once(path: str, old: str, new: str) -> None:
    file = Path(path)
    text = file.read_text()
    count = text.count(old)
    if count != 1:
        raise SystemExit(f'{path}: expected one compile-fix anchor, found {count}')
    file.write_text(text.replace(old, new, 1))

replace_once(
    'rust/crates/mukei-core/src/application_runtime/storage_import_tests.rs',
    '''            RuntimeServices {
                backend_factory: None,
                storage_importer: importer,
            },
''',
    '''            RuntimeServices {
                backend_factory: None,
                storage_importer: importer,
                storage_workspace: None,
            },
''',
)

replace_once(
    'rust/crates/mukei-core/src/ui_protocol.rs',
    '''pub struct StorageDirectoryCreatePayload {
    pub parent_node_id: String,
    pub name: String,
}
''',
    '''pub struct StorageDirectoryCreatePayload {
    /// Active Universal Storage directory that will own the new child.
    pub parent_node_id: String,
    /// User-visible directory name.
    pub name: String,
}
''',
)
replace_once(
    'rust/crates/mukei-core/src/ui_protocol.rs',
    '''pub struct StorageNodeRenamePayload {
    pub node_id: String,
    pub name: String,
}
''',
    '''pub struct StorageNodeRenamePayload {
    /// User-owned Universal Storage node identity.
    pub node_id: String,
    /// Replacement user-visible name.
    pub name: String,
}
''',
)
replace_once(
    'rust/crates/mukei-core/src/ui_protocol.rs',
    '''pub struct StorageNodePayload {
    pub node_id: String,
}
''',
    '''pub struct StorageNodePayload {
    /// User-owned Universal Storage node identity.
    pub node_id: String,
}
''',
)
replace_once(
    'rust/crates/mukei-core/src/storage/workspace_service.rs',
    '        || trimmed.as_bytes().len() > MAX_DIRECTORY_NAME_BYTES\n',
    '        || trimmed.len() > MAX_DIRECTORY_NAME_BYTES\n',
)

print('storage workspace M1 compile follow-up patch applied')
