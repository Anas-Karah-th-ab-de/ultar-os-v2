//! Where segment files live: a directory, or memory with a byte budget
//! (to prove the bounds and to simulate a full disk).

use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

/// An open segment file.
pub trait SegmentFile: Send {
    /// Appends `data`. On an error a part of `data` may have been written.
    fn append(&mut self, data: &[u8]) -> io::Result<()>;
    /// Cuts the file to `len` bytes (to remove a partly written record).
    fn truncate(&mut self, len: u64) -> io::Result<()>;
    /// Forces the content to stable storage.
    fn sync(&mut self) -> io::Result<()>;
}

/// The directory of the log.
pub trait Storage: Send {
    /// Names and sizes of all files.
    fn list(&self) -> io::Result<Vec<(String, u64)>>;
    /// Creates a new, empty file; fails if it exists.
    fn create(&mut self, name: &str) -> io::Result<Box<dyn SegmentFile>>;
    /// Opens an existing file for appending.
    fn open_append(&mut self, name: &str) -> io::Result<Box<dyn SegmentFile>>;
    /// Reads a whole file, refusing files larger than `max` bytes.
    fn read(&self, name: &str, max: u64) -> io::Result<Vec<u8>>;
    /// Removes a file.
    fn remove(&mut self, name: &str) -> io::Result<()>;
    /// Replaces a small file atomically (temporary file, sync, rename).
    fn write_atomic(&mut self, name: &str, data: &[u8]) -> io::Result<()>;
}

fn too_large() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "file larger than allowed")
}

/// A real directory. Files are created with mode `0600`.
#[derive(Debug, Clone)]
pub struct DirStorage {
    dir: PathBuf,
}

impl DirStorage {
    /// Uses `dir`, creating it (mode `0700` under the service's umask) if
    /// needed.
    pub fn open(dir: &Path) -> io::Result<Self> {
        std::fs::create_dir_all(dir)?;
        Ok(Self {
            dir: dir.to_owned(),
        })
    }

    fn path(&self, name: &str) -> io::Result<PathBuf> {
        // Names come from this crate, but a name with a separator must
        // never leave the directory.
        if name.is_empty() || name.contains('/') || name.starts_with('.') {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "bad file name"));
        }
        Ok(self.dir.join(name))
    }

    fn sync_dir(&self) -> io::Result<()> {
        File::open(&self.dir)?.sync_all()
    }
}

struct DirFile(File);

impl SegmentFile for DirFile {
    fn append(&mut self, data: &[u8]) -> io::Result<()> {
        self.0.write_all(data)
    }

    fn truncate(&mut self, len: u64) -> io::Result<()> {
        self.0.set_len(len)
    }

    fn sync(&mut self) -> io::Result<()> {
        self.0.sync_data()
    }
}

impl Storage for DirStorage {
    fn list(&self) -> io::Result<Vec<(String, u64)>> {
        let mut files = Vec::new();
        for entry in std::fs::read_dir(&self.dir)? {
            let entry = entry?;
            let meta = entry.metadata()?;
            if let (true, Some(name)) = (meta.is_file(), entry.file_name().to_str()) {
                files.push((name.to_owned(), meta.len()));
            }
        }
        files.sort();
        Ok(files)
    }

    fn create(&mut self, name: &str) -> io::Result<Box<dyn SegmentFile>> {
        let file = OpenOptions::new()
            .append(true)
            .create_new(true)
            .mode(0o600)
            .open(self.path(name)?)?;
        self.sync_dir()?;
        Ok(Box::new(DirFile(file)))
    }

    fn open_append(&mut self, name: &str) -> io::Result<Box<dyn SegmentFile>> {
        let file = OpenOptions::new().append(true).open(self.path(name)?)?;
        Ok(Box::new(DirFile(file)))
    }

    fn read(&self, name: &str, max: u64) -> io::Result<Vec<u8>> {
        let path = self.path(name)?;
        if std::fs::metadata(&path)?.len() > max {
            return Err(too_large());
        }
        let mut data = Vec::new();
        io::Read::read_to_end(&mut io::Read::take(File::open(path)?, max + 1), &mut data)?;
        if data.len() as u64 > max {
            return Err(too_large());
        }
        Ok(data)
    }

    fn remove(&mut self, name: &str) -> io::Result<()> {
        std::fs::remove_file(self.path(name)?)?;
        self.sync_dir()
    }

    fn write_atomic(&mut self, name: &str, data: &[u8]) -> io::Result<()> {
        let target = self.path(name)?;
        let temp = self.dir.join(format!("{name}.tmp"));
        let result = (|| {
            let mut file = OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o600)
                .open(&temp)?;
            file.write_all(data)?;
            file.sync_all()?;
            std::fs::rename(&temp, &target)?;
            self.sync_dir()
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(&temp);
        }
        result
    }
}

#[derive(Default)]
struct MemState {
    files: BTreeMap<String, Vec<u8>>,
    capacity: u64,
    fail_everything: bool,
    writes: u64,
    syncs: u64,
    high_water: u64,
}

impl MemState {
    fn used(&self) -> u64 {
        self.files.values().map(|data| data.len() as u64).sum()
    }
}

/// Storage in memory with a byte budget: a "disk" of `capacity` bytes.
/// Writing beyond the budget stores the part that fits and then fails
/// like a full disk does. Clones share the same disk, so a test can
/// inspect and change it while a store uses it.
#[derive(Clone, Default)]
pub struct MemStorage {
    state: Arc<Mutex<MemState>>,
}

fn full() -> io::Error {
    io::Error::new(io::ErrorKind::StorageFull, "no space left on device")
}

impl MemStorage {
    /// A disk of `capacity` bytes.
    pub fn with_capacity(capacity: u64) -> Self {
        let storage = Self::default();
        storage.lock().capacity = capacity;
        storage
    }

    fn lock(&self) -> MutexGuard<'_, MemState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Changes the size of the disk.
    pub fn set_capacity(&self, capacity: u64) {
        self.lock().capacity = capacity;
    }

    /// Makes every operation fail (an unplugged or broken disk).
    pub fn set_broken(&self, broken: bool) {
        self.lock().fail_everything = broken;
    }

    /// Bytes in use.
    pub fn used(&self) -> u64 {
        self.lock().used()
    }

    /// Most bytes ever in use.
    pub fn high_water(&self) -> u64 {
        self.lock().high_water
    }

    /// Number of `sync` calls on segment files.
    pub fn syncs(&self) -> u64 {
        self.lock().syncs
    }

    /// A copy of a file.
    pub fn file(&self, name: &str) -> Option<Vec<u8>> {
        self.lock().files.get(name).cloned()
    }

    /// Replaces a file's content (tampering in tests).
    pub fn put(&self, name: &str, data: Vec<u8>) {
        self.lock().files.insert(name.to_owned(), data);
    }

    /// Names of all files.
    pub fn names(&self) -> Vec<String> {
        self.lock().files.keys().cloned().collect()
    }
}

struct MemFile {
    state: Arc<Mutex<MemState>>,
    name: String,
}

impl MemFile {
    fn lock(&self) -> MutexGuard<'_, MemState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

fn broken() -> io::Error {
    io::Error::other("storage broken")
}

impl SegmentFile for MemFile {
    fn append(&mut self, data: &[u8]) -> io::Result<()> {
        let mut state = self.lock();
        if state.fail_everything {
            return Err(broken());
        }
        let free = state.capacity.saturating_sub(state.used());
        let fits = usize::try_from(free).unwrap_or(usize::MAX).min(data.len());
        state.writes += 1;
        let name = self.name.clone();
        let file = state.files.get_mut(&name).ok_or_else(broken)?;
        file.extend_from_slice(&data[..fits]);
        state.high_water = state.high_water.max(state.used());
        if fits < data.len() {
            return Err(full());
        }
        Ok(())
    }

    fn truncate(&mut self, len: u64) -> io::Result<()> {
        let mut state = self.lock();
        if state.fail_everything {
            return Err(broken());
        }
        let name = self.name.clone();
        let file = state.files.get_mut(&name).ok_or_else(broken)?;
        file.truncate(usize::try_from(len).unwrap_or(usize::MAX));
        Ok(())
    }

    fn sync(&mut self) -> io::Result<()> {
        let mut state = self.lock();
        if state.fail_everything {
            return Err(broken());
        }
        state.syncs += 1;
        Ok(())
    }
}

impl Storage for MemStorage {
    fn list(&self) -> io::Result<Vec<(String, u64)>> {
        let state = self.lock();
        if state.fail_everything {
            return Err(broken());
        }
        Ok(state
            .files
            .iter()
            .map(|(name, data)| (name.clone(), data.len() as u64))
            .collect())
    }

    fn create(&mut self, name: &str) -> io::Result<Box<dyn SegmentFile>> {
        let mut state = self.lock();
        if state.fail_everything {
            return Err(broken());
        }
        if state.files.contains_key(name) {
            return Err(io::ErrorKind::AlreadyExists.into());
        }
        state.files.insert(name.to_owned(), Vec::new());
        Ok(Box::new(MemFile {
            state: self.state.clone(),
            name: name.to_owned(),
        }))
    }

    fn open_append(&mut self, name: &str) -> io::Result<Box<dyn SegmentFile>> {
        let state = self.lock();
        if state.fail_everything || !state.files.contains_key(name) {
            return Err(io::ErrorKind::NotFound.into());
        }
        Ok(Box::new(MemFile {
            state: self.state.clone(),
            name: name.to_owned(),
        }))
    }

    fn read(&self, name: &str, max: u64) -> io::Result<Vec<u8>> {
        let state = self.lock();
        if state.fail_everything {
            return Err(broken());
        }
        let data = state
            .files
            .get(name)
            .ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))?;
        if data.len() as u64 > max {
            return Err(too_large());
        }
        Ok(data.clone())
    }

    fn remove(&mut self, name: &str) -> io::Result<()> {
        let mut state = self.lock();
        if state.fail_everything {
            return Err(broken());
        }
        state
            .files
            .remove(name)
            .map(|_| ())
            .ok_or_else(|| io::ErrorKind::NotFound.into())
    }

    fn write_atomic(&mut self, name: &str, data: &[u8]) -> io::Result<()> {
        let mut state = self.lock();
        if state.fail_everything {
            return Err(broken());
        }
        let old = state.files.get(name).map_or(0, |file| file.len() as u64);
        if state.used() - old + data.len() as u64 > state.capacity {
            // Atomic: on failure the old content stays.
            return Err(full());
        }
        state.files.insert(name.to_owned(), data.to_vec());
        state.high_water = state.high_water.max(state.used());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn temp(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ulogd-st-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn exercise(storage: &mut dyn Storage) {
        let mut file = storage.create("a.seg").unwrap();
        file.append(b"hello ").unwrap();
        file.append(b"world").unwrap();
        file.sync().unwrap();
        assert!(storage.create("a.seg").is_err());
        assert_eq!(storage.read("a.seg", 100).unwrap(), b"hello world");
        assert!(storage.read("a.seg", 5).is_err());
        file.truncate(5).unwrap();
        file.append(b"!").unwrap();
        drop(file);
        let mut again = storage.open_append("a.seg").unwrap();
        again.append(b"?").unwrap();
        assert_eq!(storage.read("a.seg", 100).unwrap(), b"hello!?");
        storage.write_atomic("head", b"one").unwrap();
        storage.write_atomic("head", b"two").unwrap();
        assert_eq!(storage.read("head", 10).unwrap(), b"two");
        assert_eq!(
            storage.list().unwrap(),
            vec![("a.seg".to_owned(), 7), ("head".to_owned(), 3)]
        );
        storage.remove("a.seg").unwrap();
        assert!(storage.remove("a.seg").is_err());
        assert!(storage.open_append("a.seg").is_err());
        assert!(storage.read("a.seg", 10).is_err());
    }

    #[test]
    fn directory_storage_behaves_and_files_are_private() {
        let dir = temp("dir");
        let mut storage = DirStorage::open(&dir).unwrap();
        storage.create("p.seg").unwrap();
        let mode = std::fs::metadata(dir.join("p.seg"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o077, 0);
        storage.remove("p.seg").unwrap();
        exercise(&mut storage);
        for bad in ["", "../x", "a/b", ".hidden"] {
            assert!(storage.create(bad).is_err(), "{bad:?}");
            assert!(storage.read(bad, 10).is_err());
        }
        // No temporary file is left behind.
        assert_eq!(storage.list().unwrap().len(), 1);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn memory_storage_behaves_the_same() {
        let mut storage = MemStorage::with_capacity(1 << 20);
        exercise(&mut storage);
        assert_eq!(storage.names(), ["head"]);
        assert!(storage.syncs() >= 1);
    }

    #[test]
    fn a_full_disk_writes_what_fits_and_then_fails() {
        let mut storage = MemStorage::with_capacity(10);
        let mut file = storage.create("f").unwrap();
        file.append(b"12345678").unwrap();
        let error = file.append(b"abcdef").unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::StorageFull);
        // The part that fitted is there, as on a real disk.
        assert_eq!(storage.file("f").unwrap(), b"12345678ab");
        file.truncate(8).unwrap();
        assert!(storage.write_atomic("head", b"xyz").is_err());
        assert_eq!(storage.file("head"), None);
        storage.set_capacity(100);
        file.append(b"abcdef").unwrap();
        assert_eq!((storage.used(), storage.high_water()), (14, 14));
        storage.set_broken(true);
        assert!(file.append(b"x").is_err() && file.sync().is_err() && file.truncate(0).is_err());
        assert!(storage.list().is_err() && storage.create("g").is_err());
        storage.set_broken(false);
        storage.put("f", b"tampered".to_vec());
        assert_eq!(storage.read("f", 100).unwrap(), b"tampered");
    }
}
