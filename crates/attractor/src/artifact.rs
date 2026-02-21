use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::RwLock;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{AttractorError, Result};

/// Threshold above which artifacts are stored on disk instead of in memory (100KB).
const FILE_BACKING_THRESHOLD: usize = 100 * 1024;

/// Metadata about a stored artifact.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactInfo {
    pub id: String,
    pub name: String,
    pub size_bytes: usize,
    pub stored_at: DateTime<Utc>,
    pub is_file_backed: bool,
}

/// Storage for artifacts, either held in memory or backed by files on disk.
enum StoredData {
    InMemory(Value),
    FileBacked(PathBuf),
}

/// Named, typed storage for large stage outputs.
pub struct ArtifactStore {
    base_dir: Option<PathBuf>,
    artifacts: RwLock<HashMap<String, (ArtifactInfo, StoredData)>>,
}

impl std::fmt::Debug for ArtifactStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ArtifactStore")
            .field("base_dir", &self.base_dir)
            .finish_non_exhaustive()
    }
}

impl ArtifactStore {
    #[must_use]
    pub fn new(base_dir: Option<PathBuf>) -> Self {
        Self {
            base_dir,
            artifacts: RwLock::new(HashMap::new()),
        }
    }

    /// Store an artifact. Large artifacts with a configured `base_dir` are written to disk.
    ///
    /// # Errors
    ///
    /// Returns an error if serialization fails or the file cannot be written.
    ///
    /// # Panics
    ///
    /// Panics if the internal lock is poisoned.
    pub fn store(&self, id: impl Into<String>, name: impl Into<String>, data: Value) -> Result<ArtifactInfo> {
        let id = id.into();
        let name = name.into();
        let serialized = serde_json::to_string(&data)
            .map_err(|e| AttractorError::Engine(format!("artifact serialize failed: {e}")))?;
        let size_bytes = serialized.len();

        let is_file_backed = size_bytes > FILE_BACKING_THRESHOLD && self.base_dir.is_some();

        let stored = if is_file_backed {
            let base = self.base_dir.as_ref().expect("base_dir checked above");
            let artifacts_dir = base.join("artifacts");
            std::fs::create_dir_all(&artifacts_dir)?;
            let file_path = artifacts_dir.join(format!("{id}.json"));
            std::fs::write(&file_path, &serialized)?;
            StoredData::FileBacked(file_path)
        } else {
            StoredData::InMemory(data)
        };

        let info = ArtifactInfo {
            id: id.clone(),
            name,
            size_bytes,
            stored_at: Utc::now(),
            is_file_backed,
        };

        self.artifacts
            .write()
            .expect("artifact lock poisoned")
            .insert(id, (info.clone(), stored));

        Ok(info)
    }

    /// Retrieve an artifact's data by ID.
    ///
    /// # Errors
    ///
    /// Returns an error if the artifact is not found or cannot be read from disk.
    ///
    /// # Panics
    ///
    /// Panics if the internal lock is poisoned.
    pub fn retrieve(&self, id: &str) -> Result<Value> {
        let guard = self.artifacts.read().expect("artifact lock poisoned");
        let (_, stored) = guard
            .get(id)
            .ok_or_else(|| AttractorError::Engine(format!("artifact not found: {id}")))?;

        match stored {
            StoredData::InMemory(v) => Ok(v.clone()),
            StoredData::FileBacked(path) => {
                let path = path.clone();
                drop(guard);
                let data = std::fs::read_to_string(&path).map_err(|e| {
                    AttractorError::Engine(format!(
                        "failed to read file-backed artifact {id}: {e}"
                    ))
                })?;
                serde_json::from_str(&data).map_err(|e| {
                    AttractorError::Engine(format!(
                        "failed to deserialize file-backed artifact {id}: {e}"
                    ))
                })
            }
        }
    }

    /// Check if an artifact exists.
    ///
    /// # Panics
    ///
    /// Panics if the internal lock is poisoned.
    pub fn has(&self, id: &str) -> bool {
        self.artifacts
            .read()
            .expect("artifact lock poisoned")
            .contains_key(id)
    }

    /// List all artifact metadata.
    ///
    /// # Panics
    ///
    /// Panics if the internal lock is poisoned.
    #[must_use]
    pub fn list(&self) -> Vec<ArtifactInfo> {
        self.artifacts
            .read()
            .expect("artifact lock poisoned")
            .values()
            .map(|(info, _)| info.clone())
            .collect()
    }

    /// Remove an artifact by ID. Also deletes file-backed data from disk.
    ///
    /// # Panics
    ///
    /// Panics if the internal lock is poisoned.
    pub fn remove(&self, id: &str) {
        let mut guard = self.artifacts.write().expect("artifact lock poisoned");
        if let Some((_, StoredData::FileBacked(path))) = guard.remove(id) {
            let _ = std::fs::remove_file(path);
        }
    }

    /// Remove all artifacts. Also deletes file-backed data from disk.
    ///
    /// # Panics
    ///
    /// Panics if the internal lock is poisoned.
    pub fn clear(&self) {
        let mut guard = self.artifacts.write().expect("artifact lock poisoned");
        for (_, stored) in guard.values() {
            if let StoredData::FileBacked(path) = stored {
                let _ = std::fs::remove_file(path);
            }
        }
        guard.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn store_and_retrieve_small_artifact() {
        let store = ArtifactStore::new(None);
        let data = serde_json::json!({"result": "ok"});
        let info = store.store("art1", "test artifact", data.clone()).unwrap();

        assert_eq!(info.id, "art1");
        assert_eq!(info.name, "test artifact");
        assert!(!info.is_file_backed);
        assert!(info.size_bytes > 0);

        let retrieved = store.retrieve("art1").unwrap();
        assert_eq!(retrieved, data);
    }

    #[test]
    fn retrieve_nonexistent() {
        let store = ArtifactStore::new(None);
        assert!(store.retrieve("missing").is_err());
    }

    #[test]
    fn has_artifact() {
        let store = ArtifactStore::new(None);
        assert!(!store.has("x"));
        store.store("x", "x", serde_json::json!(1)).unwrap();
        assert!(store.has("x"));
    }

    #[test]
    fn list_artifacts() {
        let store = ArtifactStore::new(None);
        store.store("a", "alpha", serde_json::json!(1)).unwrap();
        store.store("b", "beta", serde_json::json!(2)).unwrap();
        let list = store.list();
        assert_eq!(list.len(), 2);
    }

    #[test]
    fn remove_artifact() {
        let store = ArtifactStore::new(None);
        store.store("r", "remove me", serde_json::json!(1)).unwrap();
        assert!(store.has("r"));
        store.remove("r");
        assert!(!store.has("r"));
    }

    #[test]
    fn clear_artifacts() {
        let store = ArtifactStore::new(None);
        store.store("a", "a", serde_json::json!(1)).unwrap();
        store.store("b", "b", serde_json::json!(2)).unwrap();
        assert_eq!(store.list().len(), 2);
        store.clear();
        assert!(store.list().is_empty());
    }

    #[test]
    fn file_backed_storage() {
        let dir = tempfile::tempdir().unwrap();
        let store = ArtifactStore::new(Some(dir.path().to_path_buf()));

        // Create data larger than the 100KB threshold
        let large_string = "x".repeat(FILE_BACKING_THRESHOLD + 1);
        let data = serde_json::json!(large_string);

        let info = store.store("big", "large artifact", data.clone()).unwrap();
        assert!(info.is_file_backed);
        assert!(info.size_bytes > FILE_BACKING_THRESHOLD);

        let retrieved = store.retrieve("big").unwrap();
        assert_eq!(retrieved, data);
    }

    #[test]
    fn file_backed_remove_deletes_file() {
        let dir = tempfile::tempdir().unwrap();
        let store = ArtifactStore::new(Some(dir.path().to_path_buf()));

        let large_string = "x".repeat(FILE_BACKING_THRESHOLD + 1);
        let data = serde_json::json!(large_string);
        store.store("big", "large", data).unwrap();

        let file_path = dir.path().join("artifacts").join("big.json");
        assert!(file_path.exists());

        store.remove("big");
        assert!(!file_path.exists());
    }

    #[test]
    fn small_artifact_stays_in_memory_even_with_base_dir() {
        let dir = tempfile::tempdir().unwrap();
        let store = ArtifactStore::new(Some(dir.path().to_path_buf()));

        let data = serde_json::json!({"small": true});
        let info = store.store("small", "tiny", data).unwrap();
        assert!(!info.is_file_backed);
    }

    #[test]
    fn no_file_backing_without_base_dir() {
        let store = ArtifactStore::new(None);

        let large_string = "x".repeat(FILE_BACKING_THRESHOLD + 1);
        let data = serde_json::json!(large_string);
        let info = store.store("big", "large", data).unwrap();
        assert!(!info.is_file_backed);
    }
}
