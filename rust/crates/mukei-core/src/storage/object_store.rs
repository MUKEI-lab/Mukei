//! Crash-safe encrypted immutable object storage.
//!
//! Plaintext is never written to the object-store directory. Callers provide a
//! platform-backed cipher (Android Keystore in production); this module owns
//! hashing, deduplication, opaque paths, atomic publication, fsync ordering, and
//! fail-closed verification.

use crate::storage::universal::StorageObjectId;
use aes_gcm::{
    aead::{Aead, Payload},
    Aes256Gcm, KeyInit, Nonce,
};
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use zeroize::{Zeroize, Zeroizing};

const OBJECT_FORMAT_MAGIC: &[u8; 8] = b"MUKEIOB1";
const SHA256_LEN: usize = 32;
const AES_GCM_NONCE_LEN: usize = 12;
const AES_GCM_TAG_LEN: usize = 16;
const AES_GCM_CIPHER_VERSION: u32 = 1;
const MAX_ENCODED_OBJECT_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoredObject {
    pub object_id: StorageObjectId,
    pub plaintext_sha256: [u8; SHA256_LEN],
    pub plaintext_size: u64,
    pub encrypted_size: u64,
    pub relative_path: PathBuf,
    pub deduplicated: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum ObjectStoreError {
    #[error("object-store I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("object encryption failed: {0}")]
    Encryption(String),
    #[error("object decryption failed: {0}")]
    Decryption(String),
    #[error("encrypted object exceeds the supported size limit")]
    ObjectTooLarge,
    #[error("encrypted object is malformed")]
    MalformedObject,
    #[error("encrypted object failed plaintext integrity verification")]
    IntegrityMismatch,
    #[error("existing deduplicated object is missing or corrupt")]
    CorruptDeduplicatedObject,
}

/// Encryption boundary. Production implementations must use an authenticated
/// cipher and a non-exportable platform key. `seal` must return nonce/tag data
/// inside its output; object-store code treats the result as opaque bytes.
pub trait ObjectCipher: Send + Sync {
    fn version(&self) -> u32;
    fn seal(&self, plaintext: &[u8], associated_data: &[u8]) -> Result<Vec<u8>, String>;
    fn open(&self, ciphertext: &[u8], associated_data: &[u8]) -> Result<Vec<u8>, String>;
}

/// AES-256-GCM object cipher backed by one process-resident data-encryption key.
///
/// Android production boot unwraps this key through Android Keystore and passes
/// the raw bytes across JNI exactly once. The key is zeroized when the runtime
/// drops; every object receives a fresh random 96-bit nonce and authenticates
/// the immutable object's digest/size/version metadata as associated data.
pub struct Aes256GcmObjectCipher {
    key: Zeroizing<[u8; 32]>,
}

impl Aes256GcmObjectCipher {
    pub fn new(key: [u8; 32]) -> Self {
        Self {
            key: Zeroizing::new(key),
        }
    }

    fn cipher(&self) -> Result<Aes256Gcm, String> {
        Aes256Gcm::new_from_slice(&*self.key)
            .map_err(|_| "invalid AES-256-GCM key length".to_string())
    }
}

impl ObjectCipher for Aes256GcmObjectCipher {
    fn version(&self) -> u32 {
        AES_GCM_CIPHER_VERSION
    }

    fn seal(&self, plaintext: &[u8], associated_data: &[u8]) -> Result<Vec<u8>, String> {
        let mut nonce_bytes = [0_u8; AES_GCM_NONCE_LEN];
        getrandom::getrandom(&mut nonce_bytes)
            .map_err(|_| "secure random nonce generation failed".to_string())?;
        let ciphertext = self
            .cipher()?
            .encrypt(
                Nonce::from_slice(&nonce_bytes),
                Payload {
                    msg: plaintext,
                    aad: associated_data,
                },
            )
            .map_err(|_| "AES-256-GCM encryption failed".to_string())?;
        let mut sealed = Vec::with_capacity(AES_GCM_NONCE_LEN + ciphertext.len());
        sealed.extend_from_slice(&nonce_bytes);
        sealed.extend_from_slice(&ciphertext);
        nonce_bytes.zeroize();
        Ok(sealed)
    }

    fn open(&self, ciphertext: &[u8], associated_data: &[u8]) -> Result<Vec<u8>, String> {
        if ciphertext.len() < AES_GCM_NONCE_LEN + AES_GCM_TAG_LEN {
            return Err("AES-256-GCM envelope is truncated".to_string());
        }
        let (nonce, encrypted) = ciphertext.split_at(AES_GCM_NONCE_LEN);
        self.cipher()?
            .decrypt(
                Nonce::from_slice(nonce),
                Payload {
                    msg: encrypted,
                    aad: associated_data,
                },
            )
            .map_err(|_| "AES-256-GCM authentication failed".to_string())
    }
}

pub struct ImmutableObjectStore<C> {
    root: PathBuf,
    cipher: C,
}

impl<C: ObjectCipher> ImmutableObjectStore<C> {
    pub fn open(root: impl Into<PathBuf>, cipher: C) -> Result<Self, ObjectStoreError> {
        let root = root.into();
        fs::create_dir_all(&root)?;
        sync_directory(&root)?;
        Ok(Self { root, cipher })
    }

    /// Encryption generation encoded into newly published object headers.
    pub fn encryption_version(&self) -> u32 {
        self.cipher.version()
    }

    /// Encrypt and atomically publish an immutable object. The deduplication key
    /// is the complete SHA-256 digest plus plaintext size; truncated hashes are
    /// never used for identity or paths.
    pub fn put(&self, plaintext: &[u8]) -> Result<StoredObject, ObjectStoreError> {
        let digest: [u8; SHA256_LEN] = Sha256::digest(plaintext).into();
        let plaintext_size = plaintext.len() as u64;
        let relative_path = object_relative_path(&digest, plaintext_size);
        let final_path = self.root.join(&relative_path);

        if final_path.exists() {
            self.verify_existing(&final_path, &digest, plaintext_size)
                .map_err(|_| ObjectStoreError::CorruptDeduplicatedObject)?;
            return self.stored_object(digest, plaintext_size, relative_path, true);
        }

        let parent = final_path
            .parent()
            .ok_or(ObjectStoreError::MalformedObject)?;
        fs::create_dir_all(parent)?;
        sync_directory(parent)?;

        let version = self.cipher.version();
        let associated_data = associated_data(&digest, plaintext_size, version);
        let ciphertext = self
            .cipher
            .seal(plaintext, &associated_data)
            .map_err(ObjectStoreError::Encryption)?;
        let ciphertext_size = u64::try_from(ciphertext.len()).map_err(|_| ObjectStoreError::ObjectTooLarge)?;
        if ciphertext_size > MAX_ENCODED_OBJECT_BYTES {
            return Err(ObjectStoreError::ObjectTooLarge);
        }
        let encoded = encode_object(
            version,
            &digest,
            plaintext_size,
            ciphertext_size,
            &ciphertext,
        );
        let temporary_path = parent.join(format!(
            ".{}.{}.tmp",
            hex_digest(&digest),
            uuid::Uuid::new_v4()
        ));

        let write_result = (|| -> Result<bool, ObjectStoreError> {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary_path)?;
            file.write_all(&encoded)?;
            file.sync_all()?;
            drop(file);

            match publish_without_replace(&temporary_path, &final_path) {
                Ok(()) => {
                    sync_directory(parent)?;
                    Ok(false)
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    // A racing writer won. The destination was never replaced;
                    // verify the winner before accepting this as deduplication.
                    fs::remove_file(&temporary_path)?;
                    self.verify_existing(&final_path, &digest, plaintext_size)
                        .map_err(|_| ObjectStoreError::CorruptDeduplicatedObject)?;
                    Ok(true)
                }
                Err(error) => Err(ObjectStoreError::Io(error)),
            }
        })();

        if write_result.is_err() {
            let _ = fs::remove_file(&temporary_path);
        }
        let deduplicated = write_result?;
        self.stored_object(digest, plaintext_size, relative_path, deduplicated)
    }

    pub fn read_verified(&self, object: &StoredObject) -> Result<Vec<u8>, ObjectStoreError> {
        let path = self.root.join(&object.relative_path);
        let (version, digest, size, ciphertext) = read_encoded_object(&path)?;
        if digest != object.plaintext_sha256 || size != object.plaintext_size {
            return Err(ObjectStoreError::IntegrityMismatch);
        }
        let plaintext = self
            .cipher
            .open(&ciphertext, &associated_data(&digest, size, version))
            .map_err(ObjectStoreError::Decryption)?;
        verify_plaintext(&plaintext, &digest, size)?;
        Ok(plaintext)
    }

    fn stored_object(
        &self,
        digest: [u8; SHA256_LEN],
        plaintext_size: u64,
        relative_path: PathBuf,
        deduplicated: bool,
    ) -> Result<StoredObject, ObjectStoreError> {
        let encrypted_size = fs::metadata(self.root.join(&relative_path))?.len();
        Ok(StoredObject {
            object_id: StorageObjectId::new(),
            plaintext_sha256: digest,
            plaintext_size,
            encrypted_size,
            relative_path,
            deduplicated,
        })
    }

    fn verify_existing(
        &self,
        path: &Path,
        expected_digest: &[u8; SHA256_LEN],
        expected_size: u64,
    ) -> Result<(), ObjectStoreError> {
        let (version, digest, size, ciphertext) = read_encoded_object(path)?;
        if &digest != expected_digest || size != expected_size {
            return Err(ObjectStoreError::IntegrityMismatch);
        }
        let plaintext = self
            .cipher
            .open(&ciphertext, &associated_data(&digest, size, version))
            .map_err(ObjectStoreError::Decryption)?;
        verify_plaintext(&plaintext, &digest, size)
    }
}

/// Publish a fully-synced temporary object without ever replacing an existing
/// destination. `rename` is intentionally not used because POSIX rename may
/// atomically overwrite the winner of a concurrent publication race.
fn publish_without_replace(temporary_path: &Path, final_path: &Path) -> std::io::Result<()> {
    fs::hard_link(temporary_path, final_path)?;
    fs::remove_file(temporary_path)?;
    Ok(())
}

fn object_relative_path(digest: &[u8; SHA256_LEN], size: u64) -> PathBuf {
    let hex = hex_digest(digest);
    PathBuf::from(&hex[0..2])
        .join(&hex[2..4])
        .join(format!("{hex}-{size}.mobj"))
}

fn associated_data(digest: &[u8; SHA256_LEN], size: u64, version: u32) -> Vec<u8> {
    let mut output = Vec::with_capacity(OBJECT_FORMAT_MAGIC.len() + 4 + SHA256_LEN + 8);
    output.extend_from_slice(OBJECT_FORMAT_MAGIC);
    output.extend_from_slice(&version.to_be_bytes());
    output.extend_from_slice(digest);
    output.extend_from_slice(&size.to_be_bytes());
    output
}

fn encode_object(
    version: u32,
    digest: &[u8; SHA256_LEN],
    size: u64,
    ciphertext_size: u64,
    ciphertext: &[u8],
) -> Vec<u8> {
    let mut output = associated_data(digest, size, version);
    output.extend_from_slice(&ciphertext_size.to_be_bytes());
    output.extend_from_slice(ciphertext);
    output
}

fn read_encoded_object(
    path: &Path,
) -> Result<(u32, [u8; SHA256_LEN], u64, Vec<u8>), ObjectStoreError> {
    let mut file = File::open(path)?;
    let mut header = [0u8; 8 + 4 + SHA256_LEN + 8 + 8];
    file.read_exact(&mut header)
        .map_err(|_| ObjectStoreError::MalformedObject)?;
    if &header[0..8] != OBJECT_FORMAT_MAGIC {
        return Err(ObjectStoreError::MalformedObject);
    }
    let version = u32::from_be_bytes(header[8..12].try_into().unwrap());
    let digest: [u8; SHA256_LEN] = header[12..44].try_into().unwrap();
    let size = u64::from_be_bytes(header[44..52].try_into().unwrap());
    let encrypted_size = u64::from_be_bytes(header[52..60].try_into().unwrap());
    let actual_remaining = file.metadata()?.len().saturating_sub(header.len() as u64);
    if actual_remaining != encrypted_size
        || encrypted_size > MAX_ENCODED_OBJECT_BYTES
        || encrypted_size > usize::MAX as u64
    {
        return Err(ObjectStoreError::MalformedObject);
    }
    let ciphertext_size = usize::try_from(encrypted_size)
        .map_err(|_| ObjectStoreError::MalformedObject)?;
    let mut ciphertext = vec![0u8; ciphertext_size];
    file.read_exact(&mut ciphertext)
        .map_err(|_| ObjectStoreError::MalformedObject)?;
    Ok((version, digest, size, ciphertext))
}

fn verify_plaintext(
    plaintext: &[u8],
    expected_digest: &[u8; SHA256_LEN],
    expected_size: u64,
) -> Result<(), ObjectStoreError> {
    let actual_digest: [u8; SHA256_LEN] = Sha256::digest(plaintext).into();
    if plaintext.len() as u64 != expected_size || &actual_digest != expected_digest {
        return Err(ObjectStoreError::IntegrityMismatch);
    }
    Ok(())
}

fn sync_directory(path: &Path) -> Result<(), std::io::Error> {
    File::open(path)?.sync_all()
}

fn hex_digest(bytes: &[u8; SHA256_LEN]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(SHA256_LEN * 2);
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestCipher;

    impl ObjectCipher for TestCipher {
        fn version(&self) -> u32 {
            1
        }

        fn seal(&self, plaintext: &[u8], associated_data: &[u8]) -> Result<Vec<u8>, String> {
            let mask = Sha256::digest(associated_data);
            Ok(plaintext
                .iter()
                .enumerate()
                .map(|(index, byte)| byte ^ mask[index % mask.len()])
                .collect())
        }

        fn open(&self, ciphertext: &[u8], associated_data: &[u8]) -> Result<Vec<u8>, String> {
            self.seal(ciphertext, associated_data)
        }
    }

    struct OversizedCipher;

    impl ObjectCipher for OversizedCipher {
        fn version(&self) -> u32 {
            1
        }

        fn seal(&self, _plaintext: &[u8], _associated_data: &[u8]) -> Result<Vec<u8>, String> {
            Ok(vec![0; (MAX_ENCODED_OBJECT_BYTES + 1) as usize])
        }

        fn open(&self, _ciphertext: &[u8], _associated_data: &[u8]) -> Result<Vec<u8>, String> {
            unreachable!("oversized ciphertext must never be published")
        }
    }

    #[test]
    fn aes_gcm_cipher_authenticates_ciphertext_and_associated_data() {
        let cipher = Aes256GcmObjectCipher::new([0x5a; 32]);
        let plaintext = b"workspace secret";
        let aad = b"object-metadata";
        let sealed = cipher.seal(plaintext, aad).unwrap();

        assert_ne!(sealed.as_slice(), plaintext);
        assert_eq!(cipher.open(&sealed, aad).unwrap(), plaintext);

        let mut tampered = sealed.clone();
        *tampered.last_mut().unwrap() ^= 0x01;
        assert!(cipher.open(&tampered, aad).is_err());
        assert!(cipher.open(&sealed, b"different-metadata").is_err());
    }

    #[test]
    fn aes_gcm_cipher_uses_fresh_nonce_per_publication() {
        let cipher = Aes256GcmObjectCipher::new([0x33; 32]);
        let first = cipher.seal(b"same", b"same-aad").unwrap();
        let second = cipher.seal(b"same", b"same-aad").unwrap();
        assert_ne!(&first[..AES_GCM_NONCE_LEN], &second[..AES_GCM_NONCE_LEN]);
        assert_ne!(first, second);
    }

    #[test]
    fn publishes_ciphertext_and_deduplicates_without_overwrite() {
        let directory = tempfile::tempdir().unwrap();
        let store = ImmutableObjectStore::open(directory.path(), TestCipher).unwrap();
        let plaintext = b"immutable workspace document";

        let first = store.put(plaintext).unwrap();
        assert!(!first.deduplicated);
        let bytes_on_disk = fs::read(directory.path().join(&first.relative_path)).unwrap();
        assert!(!bytes_on_disk
            .windows(plaintext.len())
            .any(|window| window == plaintext));
        assert_eq!(store.read_verified(&first).unwrap(), plaintext);

        let second = store.put(plaintext).unwrap();
        assert!(second.deduplicated);
        assert_eq!(first.relative_path, second.relative_path);
    }

    #[test]
    fn rejects_oversized_ciphertext_before_filesystem_publication() {
        let directory = tempfile::tempdir().unwrap();
        let store = ImmutableObjectStore::open(directory.path(), OversizedCipher).unwrap();

        assert!(matches!(
            store.put(b"small plaintext"),
            Err(ObjectStoreError::ObjectTooLarge)
        ));
        assert!(fs::read_dir(directory.path()).unwrap().next().is_none());
    }

    #[test]
    fn no_replace_publication_preserves_existing_destination() {
        let directory = tempfile::tempdir().unwrap();
        let temporary = directory.path().join("temporary");
        let destination = directory.path().join("destination");
        fs::write(&temporary, b"new").unwrap();
        fs::write(&destination, b"winner").unwrap();

        let error = publish_without_replace(&temporary, &destination).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::AlreadyExists);
        assert_eq!(fs::read(&destination).unwrap(), b"winner");
        assert_eq!(fs::read(&temporary).unwrap(), b"new");
    }

    #[test]
    fn rejects_oversized_sparse_object_before_ciphertext_allocation() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("oversized.mobj");
        let digest = [0x42; SHA256_LEN];
        let oversized = MAX_ENCODED_OBJECT_BYTES + 1;
        let mut file = File::create(&path).unwrap();
        file.write_all(&associated_data(&digest, 1, 1)).unwrap();
        file.write_all(&oversized.to_be_bytes()).unwrap();
        file.set_len(60 + oversized).unwrap();
        file.sync_all().unwrap();

        assert!(matches!(
            read_encoded_object(&path),
            Err(ObjectStoreError::MalformedObject)
        ));
    }

    #[test]
    fn corrupted_ciphertext_fails_closed() {
        let directory = tempfile::tempdir().unwrap();
        let store = ImmutableObjectStore::open(directory.path(), TestCipher).unwrap();
        let object = store.put(b"critical data").unwrap();
        let path = directory.path().join(&object.relative_path);
        let mut bytes = fs::read(&path).unwrap();
        *bytes.last_mut().unwrap() ^= 0x7f;
        fs::write(&path, bytes).unwrap();

        assert!(matches!(
            store.read_verified(&object),
            Err(ObjectStoreError::IntegrityMismatch)
        ));
        assert!(matches!(
            store.put(b"critical data"),
            Err(ObjectStoreError::CorruptDeduplicatedObject)
        ));
    }
}
