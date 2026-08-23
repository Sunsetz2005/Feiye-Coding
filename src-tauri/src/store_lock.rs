//! Advisory file locks for App JSON store (E06 shared / multi-instance).
//!
//! CLI and a second App window may touch the same data root in `shared` mode.
//! We take an exclusive lock around read-modify-write of index files so the
//! index is not half-written, and use temp+rename for atomic replace.

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use fs2::FileExt;
use serde::de::DeserializeOwned;
use serde::Serialize;

/// How long to wait for a lock before failing with a clear error.
const LOCK_WAIT: Duration = Duration::from_secs(3);
const LOCK_POLL: Duration = Duration::from_millis(40);

/// Sidecar lock path for `foo.json` → `foo.json.lock`.
pub fn lock_path_for(target: &Path) -> PathBuf {
    let mut s = target.as_os_str().to_os_string();
    s.push(".lock");
    PathBuf::from(s)
}

/// Holds an exclusive lock until dropped.
pub struct ExclusiveLock {
    _file: File,
}

impl Drop for ExclusiveLock {
    fn drop(&mut self) {
        // Keep the sidecar inode in place. Removing it is unsafe: a waiter may
        // hold the old, now-unlinked inode while a third process creates and
        // locks a new file at the same path.
        let _ = self._file.unlock();
    }
}

/// Acquire exclusive lock for `target` (creates `target.lock`).
pub fn lock_exclusive(target: &Path) -> Result<ExclusiveLock, String> {
    let path = lock_path_for(target);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("lock dir: {e}"))?;
    }
    let file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .open(&path)
        .map_err(|e| format!("open lock {}: {e}", path.display()))?;

    let deadline = Instant::now() + LOCK_WAIT;
    loop {
        match file.try_lock_exclusive() {
            Ok(()) => {
                return Ok(ExclusiveLock { _file: file });
            }
            Err(_) if Instant::now() < deadline => {
                thread::sleep(LOCK_POLL);
            }
            Err(e) => {
                return Err(format!(
                    "LOCK_BUSY: could not lock {} within {}ms ({e})",
                    path.display(),
                    LOCK_WAIT.as_millis()
                ));
            }
        }
    }
}

/// Run `body` while holding an exclusive lock on `target`.
pub fn with_exclusive_lock<T>(
    target: &Path,
    body: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    let _lock = lock_exclusive(target)?;
    body()
}

#[cfg(not(windows))]
fn replace_file_atomic(source: &Path, target: &Path) -> Result<(), String> {
    fs::rename(source, target).map_err(|error| format!("rename into place: {error}"))
}

#[cfg(windows)]
fn replace_file_atomic(source: &Path, target: &Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::{
        MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
    };

    let source: Vec<u16> = source.as_os_str().encode_wide().chain([0]).collect();
    let target: Vec<u16> = target.as_os_str().encode_wide().chain([0]).collect();
    unsafe {
        MoveFileExW(
            PCWSTR(source.as_ptr()),
            PCWSTR(target.as_ptr()),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    }
    .map_err(|error| format!("replace into place: {error}"))
}

fn write_bytes_unlocked_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let tmp = {
        let mut p = path.as_os_str().to_os_string();
        p.push(format!(".tmp.{}", uuid::Uuid::new_v4()));
        PathBuf::from(p)
    };
    let write_result = (|| {
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&tmp)
            .map_err(|error| format!("create temp: {error}"))?;
        file.write_all(bytes)
            .map_err(|error| format!("write temp: {error}"))?;
        file.sync_all()
            .map_err(|error| format!("sync temp: {error}"))?;
        drop(file);
        replace_file_atomic(&tmp, path)?;
        #[cfg(unix)]
        if let Some(parent) = path.parent() {
            File::open(parent)
                .and_then(|directory| directory.sync_all())
                .map_err(|error| format!("sync parent directory: {error}"))?;
        }
        Ok(())
    })();
    write_result.map_err(|error| {
        let _ = fs::remove_file(&tmp);
        error
    })
}

/// Write bytes to `path` via temp file + rename under exclusive lock.
pub fn write_bytes_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    with_exclusive_lock(path, || write_bytes_unlocked_atomic(path, bytes))
}

/// Perform one JSON read-modify-write transaction while holding the target lock.
/// Missing or empty files use `default`; malformed JSON fails without overwriting it.
pub fn update_json_locked<T, R>(
    path: &Path,
    default: impl FnOnce() -> T,
    update: impl FnOnce(&mut T) -> Result<R, String>,
) -> Result<R, String>
where
    T: DeserializeOwned + Serialize,
{
    with_exclusive_lock(path, || {
        let mut value = match fs::read_to_string(path) {
            Ok(raw) if raw.trim().is_empty() => default(),
            Ok(raw) => {
                serde_json::from_str(&raw).map_err(|e| format!("parse {}: {e}", path.display()))?
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => default(),
            Err(error) => return Err(format!("read {}: {error}", path.display())),
        };
        let result = update(&mut value)?;
        let bytes = serde_json::to_vec_pretty(&value)
            .map_err(|e| format!("serialize {}: {e}", path.display()))?;
        write_bytes_unlocked_atomic(path, &bytes)?;
        Ok(result)
    })
}

/// Perform one UTF-8 text read-modify-write transaction under the target lock.
/// Missing files start empty; the replacement is committed with an atomic rename.
pub fn update_text_locked<R>(
    path: &Path,
    update: impl FnOnce(String) -> Result<(String, R), String>,
) -> Result<R, String> {
    with_exclusive_lock(path, || {
        let current = match fs::read_to_string(path) {
            Ok(raw) => raw,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(error) => return Err(format!("read {}: {error}", path.display())),
        };
        let (next, result) = update(current)?;
        write_bytes_unlocked_atomic(path, next.as_bytes())?;
        Ok(result)
    })
}

/// True if error string is a lock contention failure.
pub fn is_lock_busy(err: &str) -> bool {
    err.contains("LOCK_BUSY")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Barrier};
    use std::thread;

    fn tmp_file(name: &str) -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!(
            "grok-store-lock-{}-{}-{}",
            name,
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        p
    }

    #[test]
    fn lock_path_suffix() {
        let p = PathBuf::from("/tmp/sessions_index.json");
        assert_eq!(
            lock_path_for(&p),
            PathBuf::from("/tmp/sessions_index.json.lock")
        );
    }

    #[test]
    fn atomic_write_roundtrip() {
        let path = tmp_file("atomic.json");
        write_bytes_atomic(&path, br#"{"ok":true}"#).unwrap();
        let s = fs::read_to_string(&path).unwrap();
        assert!(s.contains("ok"));
        assert!(lock_path_for(&path).is_file());
        let _ = fs::remove_file(&path);
        let _ = fs::remove_file(lock_path_for(&path));
    }

    #[test]
    fn json_transaction_keeps_concurrent_updates() {
        let path = tmp_file("json-transaction.json");
        write_bytes_atomic(&path, b"[]").unwrap();
        let barrier = Arc::new(Barrier::new(3));
        let mut threads = Vec::new();
        for value in [1_u32, 2_u32] {
            let path = path.clone();
            let barrier = Arc::clone(&barrier);
            threads.push(thread::spawn(move || {
                barrier.wait();
                update_json_locked(&path, Vec::<u32>::new, |items| {
                    items.push(value);
                    Ok(())
                })
                .unwrap();
            }));
        }
        barrier.wait();
        for thread in threads {
            thread.join().unwrap();
        }
        let mut values: Vec<u32> =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        values.sort_unstable();
        assert_eq!(values, vec![1, 2]);
        let _ = fs::remove_file(&path);
        let _ = fs::remove_file(lock_path_for(&path));
    }

    #[test]
    fn json_transaction_does_not_replace_malformed_file() {
        let path = tmp_file("json-malformed.json");
        fs::write(&path, "{broken").unwrap();
        let result = update_json_locked(&path, Vec::<u32>::new, |items| {
            items.push(1);
            Ok(())
        });
        assert!(result.is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "{broken");
        let _ = fs::remove_file(&path);
        let _ = fs::remove_file(lock_path_for(&path));
    }

    #[test]
    fn exclusive_blocks_second_holder() {
        let path = tmp_file("block.json");
        let barrier = Arc::new(Barrier::new(2));
        let path2 = path.clone();
        let b2 = Arc::clone(&barrier);

        let t = thread::spawn(move || {
            let _lock = lock_exclusive(&path2).expect("first lock");
            b2.wait();
            // Hold long enough for the other thread to time out path.
            thread::sleep(Duration::from_millis(200));
        });

        barrier.wait();
        // Second lock should fail quickly if we shrink wait — use direct try.
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .open(lock_path_for(&path))
            .unwrap();
        let busy = file.try_lock_exclusive().is_err();
        assert!(busy, "second exclusive lock should be busy");
        t.join().unwrap();
        let _ = fs::remove_file(&path);
        let _ = fs::remove_file(lock_path_for(&path));
    }

    #[test]
    fn is_lock_busy_detects_prefix() {
        assert!(is_lock_busy("LOCK_BUSY: could not lock"));
        assert!(!is_lock_busy("write temp: disk full"));
    }
}
