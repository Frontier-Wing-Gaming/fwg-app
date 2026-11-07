//! Key/value persistence, replacing AsyncStorage (cache) and expo-secure-store (staff session).
//!
//! [`FileStore`] is fine for the content cache everywhere and for the session on desktop dev
//! builds. On Android/iOS the session store must be Keystore/Keychain-backed; the platform crate
//! provides that by implementing [`KeyValueStore`].

use std::{
    collections::HashMap,
    fs, io,
    path::{Path, PathBuf},
    sync::Mutex,
};

pub trait KeyValueStore: Send + Sync {
    fn get(&self, key: &str) -> io::Result<Option<String>>;
    fn set(&self, key: &str, value: &str) -> io::Result<()>;
    /// Deleting a missing key is not an error.
    fn delete(&self, key: &str) -> io::Result<()>;
}

/// One file per key in a directory (the app's private data dir on mobile).
#[derive(Debug, Clone)]
pub struct FileStore {
    dir: PathBuf,
}

impl FileStore {
    pub fn new(dir: impl Into<PathBuf>) -> io::Result<Self> {
        let dir = dir.into();
        fs::create_dir_all(&dir)?;
        Ok(Self { dir })
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn path(&self, key: &str) -> PathBuf {
        // Keys are things like "fwg.cache.news". Anything outside a safe set is hex-escaped so a key
        // can never name a path outside the directory.
        let mut name = String::with_capacity(key.len() + 5);
        for b in key.bytes() {
            match b {
                b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'.' | b'-' | b'_' => {
                    name.push(b as char)
                }
                _ => name.push_str(&format!("%{b:02x}")),
            }
        }
        if name.starts_with('.') {
            name.insert(0, '_');
        }
        name.push_str(".json");
        self.dir.join(name)
    }
}

impl KeyValueStore for FileStore {
    fn get(&self, key: &str) -> io::Result<Option<String>> {
        match fs::read_to_string(self.path(key)) {
            Ok(s) => Ok(Some(s)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e),
        }
    }

    fn set(&self, key: &str, value: &str) -> io::Result<()> {
        // Write then rename, so a crash mid-write never leaves a truncated value behind.
        let path = self.path(key);
        let tmp = path.with_extension("json.tmp");
        fs::write(&tmp, value)?;
        fs::rename(&tmp, &path)
    }

    fn delete(&self, key: &str) -> io::Result<()> {
        match fs::remove_file(self.path(key)) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        }
    }
}

/// In-memory store for tests and previews.
#[derive(Debug, Default)]
pub struct MemoryStore(Mutex<HashMap<String, String>>);

impl KeyValueStore for MemoryStore {
    fn get(&self, key: &str) -> io::Result<Option<String>> {
        Ok(self.0.lock().unwrap().get(key).cloned())
    }

    fn set(&self, key: &str, value: &str) -> io::Result<()> {
        self.0
            .lock()
            .unwrap()
            .insert(key.to_owned(), value.to_owned());
        Ok(())
    }

    fn delete(&self, key: &str) -> io::Result<()> {
        self.0.lock().unwrap().remove(key);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_store_round_trip_and_safe_names() {
        let dir = tempfile::tempdir().unwrap();
        let store = FileStore::new(dir.path()).unwrap();
        assert_eq!(store.get("fwg.cache.news").unwrap(), None);
        store.set("fwg.cache.news", "{\"a\":1}").unwrap();
        store.set("../escape", "x").unwrap();
        assert_eq!(
            store.get("fwg.cache.news").unwrap().as_deref(),
            Some("{\"a\":1}")
        );
        assert_eq!(store.get("../escape").unwrap().as_deref(), Some("x"));
        assert!(store.path("../escape").starts_with(dir.path()));
        store.delete("fwg.cache.news").unwrap();
        store.delete("fwg.cache.news").unwrap();
        assert_eq!(store.get("fwg.cache.news").unwrap(), None);
    }
}
