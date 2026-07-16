//! Strict filename admission policy for Universal Storage and chat workspaces.
//!
//! This module intentionally validates names only. Content sniffing, encoding
//! detection, size enforcement, and parser limits are separate import-pipeline
//! stages and must not be skipped because a filename is accepted here.

/// Version of the user-visible file allowlist contract.
pub const FILE_POLICY_VERSION: u32 = 1;

/// Maximum UTF-8 byte length accepted for one display filename.
pub const MAX_FILENAME_BYTES: usize = 255;

/// Allowed extensions, stored in canonical lowercase form without the leading dot.
pub const ALLOWED_EXTENSIONS: &[&str] = &[
    "ass", "bash", "bat", "cfg", "cjs", "cmd", "conf", "cs", "css", "csv", "go",
    "htm", "html", "ini", "java", "js", "json", "jsonl", "jsx", "kt", "kts",
    "local", "log", "lua", "markdown", "md", "mjs", "php", "phtml", "pl", "pm",
    "po", "pot", "ps1", "py", "pyi", "pyw", "r", "rake", "rb", "rs", "rtf",
    "scss", "sh", "sql", "srt", "svg", "swift", "text", "toml", "ts", "tsv",
    "tsx", "txt", "vtt", "yaml", "yml", "zsh",
];

/// Allowed exact filenames, stored in canonical lowercase form.
///
/// `.env` and `.gitignore` are exact names, not generally allowed extensions.
pub const ALLOWED_EXACT_NAMES: &[&str] = &[
    "readme",
    "license",
    "makefile",
    "dockerfile",
    ".gitignore",
    ".env",
];

/// Rule that admitted a filename.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FileAdmissionRule {
    /// The last extension matched the allowlist.
    Extension(&'static str),
    /// The complete filename matched an exact-name rule.
    ExactName(&'static str),
}

/// Successful filename admission result.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AllowedFileName {
    /// Trimmed user-facing name. It is metadata only and must never become a
    /// physical object-store path.
    pub display_name: String,
    /// ASCII-case-normalized name used for deterministic comparisons.
    pub normalized_name: String,
    /// Allowlist rule that admitted this name.
    pub rule: FileAdmissionRule,
}

/// Stable filename-admission failures.
#[derive(Clone, Debug, thiserror::Error, PartialEq, Eq)]
pub enum FileAdmissionError {
    /// No usable filename remained after trimming surrounding whitespace.
    #[error("filename is empty")]
    EmptyName,
    /// The filename exceeds the bounded metadata limit.
    #[error("filename exceeds {MAX_FILENAME_BYTES} UTF-8 bytes")]
    NameTooLong,
    /// The name contains traversal separators, control characters, or a dot path.
    #[error("filename contains unsafe path or control characters")]
    UnsafeName,
    /// Neither the exact filename nor the final extension is allowed.
    #[error("file type is not allowed")]
    UnsupportedFileType,
}

/// Validate a user-provided filename against the frozen Phase-1 allowlist.
///
/// Matching is ASCII case-insensitive. The final extension is authoritative,
/// so `notes.json.txt` is admitted as `.txt`, while `notes.txt.exe` is rejected.
/// This function does not trust the extension as proof of content type.
pub fn admit_file_name(name: &str) -> Result<AllowedFileName, FileAdmissionError> {
    let display_name = name.trim();
    if display_name.is_empty() {
        return Err(FileAdmissionError::EmptyName);
    }
    if display_name.len() > MAX_FILENAME_BYTES {
        return Err(FileAdmissionError::NameTooLong);
    }
    if display_name == "."
        || display_name == ".."
        || display_name.contains('/')
        || display_name.contains('\\')
        || display_name.chars().any(char::is_control)
    {
        return Err(FileAdmissionError::UnsafeName);
    }

    let normalized_name = display_name.to_ascii_lowercase();
    if let Some(exact_name) = ALLOWED_EXACT_NAMES
        .iter()
        .copied()
        .find(|candidate| *candidate == normalized_name)
    {
        return Ok(AllowedFileName {
            display_name: display_name.to_string(),
            normalized_name,
            rule: FileAdmissionRule::ExactName(exact_name),
        });
    }

    let extension = normalized_name
        .rsplit_once('.')
        .and_then(|(stem, extension)| {
            (!stem.is_empty() && !extension.is_empty()).then_some(extension)
        })
        .ok_or(FileAdmissionError::UnsupportedFileType)?;

    let allowed_extension = ALLOWED_EXTENSIONS
        .iter()
        .copied()
        .find(|candidate| *candidate == extension)
        .ok_or(FileAdmissionError::UnsupportedFileType)?;

    Ok(AllowedFileName {
        display_name: display_name.to_string(),
        normalized_name,
        rule: FileAdmissionRule::Extension(allowed_extension),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_frozen_extension_is_admitted() {
        for extension in ALLOWED_EXTENSIONS {
            let filename = format!("sample.{extension}");
            let admitted = admit_file_name(&filename).unwrap();
            assert_eq!(admitted.rule, FileAdmissionRule::Extension(extension));
        }
    }

    #[test]
    fn exact_names_are_ascii_case_insensitive() {
        for name in [
            "README",
            "license",
            "MakeFile",
            "DOCKERFILE",
            ".GITIGNORE",
            ".ENV",
        ] {
            assert!(matches!(
                admit_file_name(name).unwrap().rule,
                FileAdmissionRule::ExactName(_)
            ));
        }
    }

    #[test]
    fn final_extension_is_authoritative() {
        assert!(matches!(
            admit_file_name("archive.json.TXT").unwrap().rule,
            FileAdmissionRule::Extension("txt")
        ));
        assert_eq!(
            admit_file_name("archive.txt.exe"),
            Err(FileAdmissionError::UnsupportedFileType)
        );
    }

    #[test]
    fn env_and_gitignore_are_exact_names_only() {
        assert!(admit_file_name(".env").is_ok());
        assert!(admit_file_name(".gitignore").is_ok());
        assert_eq!(
            admit_file_name("production.env"),
            Err(FileAdmissionError::UnsupportedFileType)
        );
        assert_eq!(
            admit_file_name("project.gitignore"),
            Err(FileAdmissionError::UnsupportedFileType)
        );
        assert!(admit_file_name("production.local").is_ok());
        assert!(admit_file_name(".env.local").is_ok());
    }

    #[test]
    fn unsafe_names_are_rejected_before_extension_matching() {
        for name in [
            "../notes.txt",
            "..\\notes.txt",
            "folder/notes.txt",
            "folder\\notes.txt",
            ".",
            "..",
            "bad\u{0000}.txt",
            "bad\nname.txt",
        ] {
            assert_eq!(
                admit_file_name(name),
                Err(FileAdmissionError::UnsafeName),
                "{name:?}"
            );
        }
    }

    #[test]
    fn unsupported_and_empty_names_are_rejected() {
        assert_eq!(admit_file_name("   "), Err(FileAdmissionError::EmptyName));
        assert_eq!(
            admit_file_name("photo.png"),
            Err(FileAdmissionError::UnsupportedFileType)
        );
        assert_eq!(
            admit_file_name("report.pdf"),
            Err(FileAdmissionError::UnsupportedFileType)
        );
        assert_eq!(
            admit_file_name("document.docx"),
            Err(FileAdmissionError::UnsupportedFileType)
        );
        assert_eq!(
            admit_file_name("Dockerfile.prod"),
            Err(FileAdmissionError::UnsupportedFileType)
        );
    }

    #[test]
    fn long_names_are_rejected_by_utf8_byte_length() {
        let name = format!("{}.txt", "a".repeat(MAX_FILENAME_BYTES));
        assert_eq!(
            admit_file_name(&name),
            Err(FileAdmissionError::NameTooLong)
        );
    }

    #[test]
    fn unicode_stems_are_allowed_when_extension_is_valid() {
        let admitted = admit_file_name("नोट्स.MD").unwrap();
        assert_eq!(admitted.normalized_name, "नोट्स.md");
        assert_eq!(admitted.rule, FileAdmissionRule::Extension("md"));
    }
}
