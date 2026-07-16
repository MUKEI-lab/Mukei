//! Universal Storage and per-chat workspace domain invariants.
//!
//! This module is deliberately independent from SQLite and Android. It freezes
//! the identifiers, mandatory directory roles, access checks, and copy/reference
//! semantics that persistence and JNI layers must preserve.

use std::fmt;
use uuid::Uuid;

/// Stable user-visible name of the app-wide storage scope.
pub const UNIVERSAL_STORAGE_NAME: &str = "Universal Storage";

/// One chat owns exactly one workspace.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ChatId(String);

impl ChatId {
    pub fn parse(value: impl Into<String>) -> Result<Self, StorageDomainError> {
        let value = value.into();
        let trimmed = value.trim();
        if trimmed.is_empty() {
            return Err(StorageDomainError::EmptyChatId);
        }
        if trimmed.chars().any(char::is_control) {
            return Err(StorageDomainError::UnsafeChatId);
        }
        Ok(Self(trimmed.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ChatId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

macro_rules! uuid_id {
    ($name:ident) => {
        #[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
        pub struct $name(pub Uuid);

        impl $name {
            pub fn new() -> Self {
                Self(Uuid::new_v4())
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(formatter)
            }
        }
    };
}

uuid_id!(StorageScopeId);
uuid_id!(StorageNodeId);
uuid_id!(WorkspaceId);
uuid_id!(StorageObjectId);
uuid_id!(FileVersionId);
uuid_id!(ImportTransactionId);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StorageScopeType {
    Universal,
    Workspace { chat_id: ChatId },
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum StorageNodeKind {
    Directory,
    File,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum StorageNodeState {
    Active,
    Importing,
    Trashed,
    Quarantined,
    Deleting,
    Deleted,
}

/// System-owned directories that are automatically created for every scope.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum SystemDirectoryRole {
    ScopeRoot,
    UploadedFiles,
    GeneratedFiles,
    Drafts,
    Research,
    Exports,
    Temporary,
    Trash,
}

impl SystemDirectoryRole {
    pub const WORKSPACE_CHILDREN: [Self; 7] = [
        Self::UploadedFiles,
        Self::GeneratedFiles,
        Self::Drafts,
        Self::Research,
        Self::Exports,
        Self::Temporary,
        Self::Trash,
    ];

    pub const UNIVERSAL_CHILDREN: [Self; 1] = [Self::Trash];

    pub const fn display_name(self) -> &'static str {
        match self {
            Self::ScopeRoot => "Workspace",
            Self::UploadedFiles => "Uploaded files",
            Self::GeneratedFiles => "Generated files",
            Self::Drafts => "Drafts",
            Self::Research => "Research",
            Self::Exports => "Exports",
            Self::Temporary => "Temporary",
            Self::Trash => "Trash",
        }
    }

    pub const fn is_user_deletable(self) -> bool {
        false
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlannedDirectory {
    pub node_id: StorageNodeId,
    pub parent_node_id: Option<StorageNodeId>,
    pub role: SystemDirectoryRole,
    pub display_name: &'static str,
}

/// Deterministic plan used by persistence to atomically create a chat workspace.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkspaceLayout {
    pub workspace_id: WorkspaceId,
    pub scope_id: StorageScopeId,
    pub chat_id: ChatId,
    pub root_node_id: StorageNodeId,
    pub directories: Vec<PlannedDirectory>,
}

impl WorkspaceLayout {
    pub fn plan(chat_id: ChatId) -> Self {
        let root_node_id = StorageNodeId::new();
        let mut directories = Vec::with_capacity(1 + SystemDirectoryRole::WORKSPACE_CHILDREN.len());
        directories.push(PlannedDirectory {
            node_id: root_node_id,
            parent_node_id: None,
            role: SystemDirectoryRole::ScopeRoot,
            display_name: SystemDirectoryRole::ScopeRoot.display_name(),
        });
        directories.extend(SystemDirectoryRole::WORKSPACE_CHILDREN.map(|role| PlannedDirectory {
            node_id: StorageNodeId::new(),
            parent_node_id: Some(root_node_id),
            role,
            display_name: role.display_name(),
        }));

        Self {
            workspace_id: WorkspaceId::new(),
            scope_id: StorageScopeId::new(),
            chat_id,
            root_node_id,
            directories,
        }
    }

    pub fn directory(&self, role: SystemDirectoryRole) -> Option<&PlannedDirectory> {
        self.directories.iter().find(|entry| entry.role == role)
    }

    pub fn uploaded_files_node_id(&self) -> StorageNodeId {
        self.directory(SystemDirectoryRole::UploadedFiles)
            .expect("workspace plans always contain Uploaded files")
            .node_id
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum DuplicatePolicy {
    /// Preserve the existing entry and create `name (2).ext`, `name (3).ext`, …
    RenameNewEntry,
    /// Fail without mutating the existing entry.
    RejectNameConflict,
    /// Create a new immutable version. This requires an explicit user action.
    ReplaceWithNewVersion,
}

impl Default for DuplicatePolicy {
    fn default() -> Self {
        Self::RenameNewEntry
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ImportTarget {
    Universal { parent_node_id: StorageNodeId },
    WorkspaceUploadedFiles {
        workspace_id: WorkspaceId,
        uploaded_files_node_id: StorageNodeId,
    },
    WorkspaceDirectory {
        workspace_id: WorkspaceId,
        parent_node_id: StorageNodeId,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkspaceAccessContext {
    pub chat_id: ChatId,
    pub workspace_id: WorkspaceId,
}

impl WorkspaceAccessContext {
    pub fn authorize(
        &self,
        requested_chat_id: &ChatId,
        requested_workspace_id: WorkspaceId,
    ) -> Result<(), StorageDomainError> {
        if self.chat_id != *requested_chat_id || self.workspace_id != requested_workspace_id {
            return Err(StorageDomainError::CrossWorkspaceAccessDenied);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, thiserror::Error, PartialEq, Eq)]
pub enum StorageDomainError {
    #[error("chat id is empty")]
    EmptyChatId,
    #[error("chat id contains control characters")]
    UnsafeChatId,
    #[error("cross-workspace access denied")]
    CrossWorkspaceAccessDenied,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn chat(value: &str) -> ChatId {
        ChatId::parse(value).unwrap()
    }

    #[test]
    fn workspace_plan_contains_exactly_one_of_every_mandatory_role() {
        let plan = WorkspaceLayout::plan(chat("chat-1"));
        let roles = plan
            .directories
            .iter()
            .map(|entry| entry.role)
            .collect::<HashSet<_>>();

        assert_eq!(plan.directories.len(), 8);
        assert_eq!(roles.len(), 8);
        assert!(roles.contains(&SystemDirectoryRole::ScopeRoot));
        for role in SystemDirectoryRole::WORKSPACE_CHILDREN {
            assert!(roles.contains(&role), "missing {role:?}");
        }
    }

    #[test]
    fn uploaded_files_is_a_direct_child_of_the_workspace_root() {
        let plan = WorkspaceLayout::plan(chat("chat-1"));
        let uploaded = plan
            .directory(SystemDirectoryRole::UploadedFiles)
            .unwrap();

        assert_eq!(uploaded.display_name, "Uploaded files");
        assert_eq!(uploaded.parent_node_id, Some(plan.root_node_id));
        assert_eq!(plan.uploaded_files_node_id(), uploaded.node_id);
    }

    #[test]
    fn independently_planned_workspaces_do_not_share_identifiers() {
        let first = WorkspaceLayout::plan(chat("chat-1"));
        let second = WorkspaceLayout::plan(chat("chat-2"));

        assert_ne!(first.workspace_id, second.workspace_id);
        assert_ne!(first.scope_id, second.scope_id);
        assert_ne!(first.root_node_id, second.root_node_id);
        assert_ne!(first.uploaded_files_node_id(), second.uploaded_files_node_id());
    }

    #[test]
    fn access_context_denies_other_chats_and_workspaces() {
        let first = WorkspaceLayout::plan(chat("chat-1"));
        let second = WorkspaceLayout::plan(chat("chat-2"));
        let access = WorkspaceAccessContext {
            chat_id: first.chat_id.clone(),
            workspace_id: first.workspace_id,
        };

        assert!(access
            .authorize(&first.chat_id, first.workspace_id)
            .is_ok());
        assert_eq!(
            access.authorize(&second.chat_id, first.workspace_id),
            Err(StorageDomainError::CrossWorkspaceAccessDenied)
        );
        assert_eq!(
            access.authorize(&first.chat_id, second.workspace_id),
            Err(StorageDomainError::CrossWorkspaceAccessDenied)
        );
    }

    #[test]
    fn chat_identifiers_are_trimmed_and_control_characters_are_rejected() {
        assert_eq!(chat("  chat-1  ").as_str(), "chat-1");
        assert_eq!(ChatId::parse("  "), Err(StorageDomainError::EmptyChatId));
        assert_eq!(
            ChatId::parse("chat\n1"),
            Err(StorageDomainError::UnsafeChatId)
        );
    }

    #[test]
    fn system_directories_cannot_be_deleted_by_user_actions() {
        for role in [
            SystemDirectoryRole::ScopeRoot,
            SystemDirectoryRole::UploadedFiles,
            SystemDirectoryRole::GeneratedFiles,
            SystemDirectoryRole::Drafts,
            SystemDirectoryRole::Research,
            SystemDirectoryRole::Exports,
            SystemDirectoryRole::Temporary,
            SystemDirectoryRole::Trash,
        ] {
            assert!(!role.is_user_deletable());
        }
    }

    #[test]
    fn duplicate_policy_never_overwrites_silently() {
        assert_eq!(DuplicatePolicy::default(), DuplicatePolicy::RenameNewEntry);
    }
}
