//! Task-owned immutable JSON journal with checked names, file locking and atomic publication.
use super::*;
use serde::de::DeserializeOwned;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// Unique temporary suffixes avoid reusing an interrupted writer's incomplete file.
static TEMP_ID: AtomicU64 = AtomicU64::new(0);
/// A bounded archive entry contains evidence only, never environment variables or provider tokens.
const MAX_BYTES: u64 = 16_777_216;

/// An exclusive local store lease serializes append operations across processes.
pub(super) struct Store {
    root: PathBuf,
    _lease: File,
}

impl Store {
    /// Open the configured task-owned store; cursors never supply filesystem paths.
    pub(super) fn open() -> Outcome<Self> {
        let path = std::env::var_os("SQLolla_INSTANCE_STORE")
            .map(PathBuf::from)
            .unwrap_or_else(|| crate::process::root().join("artifacts/instances"));
        fs::create_dir_all(&path).map_err(io_error)?;
        let root = path.canonicalize().map_err(io_error)?;
        let lease = File::options()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(root.join(".lock"))
            .map_err(io_error)?;
        lease
            .try_lock()
            .map_err(|error| failure("store.busy", error.to_string()))?;
        let result = Self {
            root,
            _lease: lease,
        };
        result.directory("bundles")?;
        result.directory("cursors")?;
        Ok(result)
    }

    /// Persist immutable semantic input and compiler artifacts under their content identity.
    pub(super) fn save_bundle(&self, bundle: &Bundle) -> Outcome<String> {
        let id = fingerprint(bundle);
        let directory = self.directory(&format!("bundles/{id}"))?;
        self.write(&directory.join("bundle.json"), bundle)?;
        Ok(id)
    }

    /// Load only a hash-addressed bundle whose canonical content still matches its identity.
    pub(super) fn bundle(&self, id: &str) -> Outcome<Bundle> {
        identifier(id)?;
        let value: Bundle = self.read(&self.path(&format!("bundles/{id}/bundle.json"))?)?;
        if fingerprint(&value) != id {
            return Err(failure("store.corrupt", "Bundle identity mismatch"));
        }
        Ok(value)
    }

    /// Immutable records form a prefix; a missing ordinal cannot be silently skipped.
    pub(super) fn record(&self, id: &str, ordinal: usize) -> Outcome<Option<Record>> {
        let path = self.record_path(id, ordinal)?;
        if !path.exists() {
            return Ok(None);
        }
        self.read(&path).map(Some)
    }

    /// Publish the complete next ordinal before exposing a cursor that refers to it.
    pub(super) fn save_record(&self, id: &str, record: &Record) -> Outcome<()> {
        self.write(&self.record_path(id, record.ordinal)?, record)
    }

    /// Reopening requires a contiguous bounded journal; later records cannot conceal a missing step.
    pub(super) fn records(&self, id: &str, limit: usize) -> Outcome<Vec<Record>> {
        let mut records = Vec::new();
        let mut missing = false;
        let mut bytes = 0;
        for ordinal in 0..=limit {
            let Some(record) = self.record(id, ordinal)? else {
                missing = true;
                continue;
            };
            if missing {
                return Err(failure("store.corrupt", "Journal has an ordinal gap"));
            }
            bytes += serde_json::to_vec(&record).map_err(json_error)?.len();
            if bytes as u64 > MAX_BYTES {
                return Err(failure(
                    "resource.open_bytes",
                    "Complete journal response exceeds capacity",
                ));
            }
            records.push(record);
        }
        if records.is_empty() {
            return Err(failure(
                "store.incomplete",
                "Bundle has no committed solver step",
            ));
        }
        Ok(records)
    }

    /// Opaque cursor bytes identify a checked content-addressed descriptor, never a raw path.
    pub(super) fn save_cursor(&self, cursor: &Cursor) -> Outcome<String> {
        let id = fingerprint(cursor);
        self.write(&self.path(&format!("cursors/{id}.json"))?, cursor)?;
        Ok(id)
    }

    /// Corrupted or unknown cursors remain errors and cannot advance an enumeration.
    pub(super) fn cursor(&self, id: &str) -> Outcome<Cursor> {
        identifier(id)?;
        let value: Cursor = self.read(&self.path(&format!("cursors/{id}.json"))?)?;
        if fingerprint(&value) != id {
            return Err(failure("cursor.invalid", "Cursor digest mismatch"));
        }
        identifier(&value.bundle_id)?;
        Ok(value)
    }

    /// Record filenames are generated only from validated identity and bounded ordinal.
    fn record_path(&self, id: &str, ordinal: usize) -> Outcome<PathBuf> {
        identifier(id)?;
        if ordinal > MAX_INSTANCES {
            return Err(failure("cursor.invalid", "Ordinal exceeds capacity"));
        }
        self.path(&format!("bundles/{id}/{ordinal:04}.json"))
    }

    /// Reject namespace redirection outside the canonical task store.
    fn directory(&self, relative: &str) -> Outcome<PathBuf> {
        let path = self.root.join(relative);
        fs::create_dir_all(&path).map_err(io_error)?;
        let actual = path.canonicalize().map_err(io_error)?;
        if !actual.starts_with(&self.root) {
            return Err(failure("store.path", "Store directory escapes root"));
        }
        Ok(actual)
    }

    /// Validate parent resolution and refuse file symlinks before reading or publishing.
    fn path(&self, relative: &str) -> Outcome<PathBuf> {
        let path = self.root.join(relative);
        let parent = path
            .parent()
            .ok_or_else(|| failure("store.path", "Missing parent"))?
            .canonicalize()
            .map_err(io_error)?;
        if !parent.starts_with(&self.root) {
            return Err(failure("store.path", "Path escapes store"));
        }
        if path.exists()
            && fs::symlink_metadata(&path)
                .map_err(io_error)?
                .file_type()
                .is_symlink()
        {
            return Err(failure("store.path", "Archive symlink refused"));
        }
        Ok(path)
    }

    /// Check serialization integrity before deserializing versioned typed evidence.
    fn read<T: DeserializeOwned>(&self, path: &Path) -> Outcome<T> {
        let mut bytes = Vec::new();
        File::open(path)
            .map_err(io_error)?
            .take(MAX_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(io_error)?;
        if bytes.len() as u64 > MAX_BYTES {
            return Err(failure("resource.archive_bytes", "Archive entry too large"));
        }
        let envelope: Value = serde_json::from_slice(&bytes).map_err(json_error)?;
        if envelope["schema"] != "sqlolla.archive_entry.v1"
            || envelope["sha256"] != fingerprint(&envelope["payload"])
        {
            return Err(failure("store.corrupt", "Archive digest/schema mismatch"));
        }
        serde_json::from_value(envelope["payload"].clone()).map_err(json_error)
    }

    /// Existing committed content is immutable; repeat publication is byte-content idempotent.
    fn write<T: Serialize>(&self, path: &Path, value: &T) -> Outcome<()> {
        let payload = serde_json::to_value(value).map_err(json_error)?;
        if path.exists() {
            let old: Value = self.read(path)?;
            if old != payload {
                return Err(failure("store.conflict", "Committed content differs"));
            }
            return Ok(());
        }
        let envelope = json!({"schema":"sqlolla.archive_entry.v1","sha256":fingerprint(&payload),"payload":payload});
        let bytes = serde_json::to_vec(&envelope).map_err(json_error)?;
        if bytes.len() as u64 > MAX_BYTES {
            return Err(failure("resource.archive_bytes", "Archive entry too large"));
        }
        atomic_publish(path, &bytes)
    }
}

/// Only lowercase SHA256 identities can participate in store path construction.
fn identifier(value: &str) -> Outcome<()> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(failure("cursor.invalid", "Expected opaque SHA256 identity"));
    }
    Ok(())
}

/// Flush file content before same-directory rename; incomplete temporary files are never records.
fn atomic_publish(path: &Path, bytes: &[u8]) -> Outcome<()> {
    let suffix = TEMP_ID.fetch_add(1, Ordering::Relaxed);
    let time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|error| failure("store.clock", error.to_string()))?
        .as_nanos();
    let temporary = path.with_extension(format!("tmp-{}-{time}-{suffix}", std::process::id()));
    let mut file = File::create_new(&temporary).map_err(io_error)?;
    file.write_all(bytes).map_err(io_error)?;
    file.sync_all().map_err(io_error)?;
    drop(file);
    fs::rename(&temporary, path).map_err(io_error)?;
    #[cfg(unix)]
    File::open(path.parent().unwrap())
        .map_err(io_error)?
        .sync_all()
        .map_err(io_error)?;
    Ok(())
}

/// Filesystem errors retain their own channel, never a solver or safety status.
fn io_error(error: std::io::Error) -> Failure {
    failure("store.io", error.to_string())
}
