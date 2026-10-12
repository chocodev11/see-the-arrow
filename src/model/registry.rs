//! Live arrows, one JSON file each, next to a lock file the arrow holds for its whole life. A lock
//! the OS released means the process is gone, whatever happened to its pid since.

use std::fs::{self, File, TryLockError};
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use super::owner::ArrowOwner;

/// One live arrow, written to `<dir>/<pid>.json` once it shows.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArrowRecord {
    pub direction: String,
    pub display: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner: Option<ArrowOwner>,
    pub pid: u32,
    /// The sign in global input units: x, y, width, height.
    pub sign_frame: [f64; 4],
    /// Unix milliseconds.
    pub started_at: u64,
    /// What the arrow marks in global input units; other arrows keep their signs off it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_frame: Option<[f64; 4]>,
    pub target_x: f64,
    pub target_y: f64,
    pub text: String,
}

pub fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_millis() as u64)
}

pub struct Registry {
    pub dir: PathBuf,
}

impl Registry {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    fn json(&self, pid: u32) -> PathBuf {
        self.dir.join(format!("{pid}.json"))
    }

    fn lock(&self, pid: u32) -> PathBuf {
        self.dir.join(format!("{pid}.lock"))
    }

    /// Takes the arrow's lock; keep the file open for as long as the arrow lives.
    pub fn claim(&self, pid: u32) -> io::Result<File> {
        fs::create_dir_all(&self.dir)?;
        let file = File::create(self.lock(pid))?;
        file.lock()?;
        Ok(file)
    }

    /// Replaces the record atomically, so a reader never sees half a file.
    pub fn write(&self, record: &ArrowRecord) -> io::Result<()> {
        let temporary = self.dir.join(format!("{}.json.tmp", record.pid));
        fs::write(&temporary, serde_json::to_vec(record)?)?;
        fs::rename(temporary, self.json(record.pid))
    }

    pub fn remove(&self, pid: u32) {
        let _ = fs::remove_file(self.json(pid));
        let _ = fs::remove_file(self.lock(pid));
    }

    /// True while the arrow's process holds its lock.
    pub fn is_alive(&self, pid: u32) -> bool {
        is_locked(&self.lock(pid))
    }

    /// Live arrows, oldest first. Files of arrows whose lock is free are removed.
    pub fn live(&self) -> Vec<ArrowRecord> {
        let Ok(entries) = fs::read_dir(&self.dir) else {
            return Vec::new();
        };
        let mut records: Vec<ArrowRecord> = entries
            .flatten()
            .filter_map(|entry| {
                let path = entry.path();
                let pid: u32 = path
                    .file_name()?
                    .to_str()?
                    .strip_suffix(".json")?
                    .parse()
                    .ok()?;
                if !self.is_alive(pid) {
                    self.remove(pid);
                    return None;
                }
                serde_json::from_slice(&fs::read(&path).ok()?).ok()
            })
            .collect();
        records.sort_by_key(|record| record.started_at);
        records
    }

    /// Returns once none of `pids` holds its lock any more, at most after `limit`.
    pub fn wait_until_gone(&self, pids: &[u32], limit: Duration) {
        let deadline = Instant::now() + limit;
        while Instant::now() < deadline && pids.iter().any(|pid| self.is_alive(*pid)) {
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}

fn is_locked(path: &Path) -> bool {
    let Ok(file) = File::open(path) else {
        return false;
    };
    matches!(file.try_lock_shared(), Err(TryLockError::WouldBlock))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn registry(name: &str) -> Registry {
        Registry::new(
            std::env::temp_dir().join(format!("bigarrow-tests-{}-{name}", std::process::id())),
        )
    }

    fn record(pid: u32) -> ArrowRecord {
        ArrowRecord {
            direction: "top".into(),
            display: 1,
            owner: None,
            pid,
            sign_frame: [0.0, 0.0, 10.0, 10.0],
            started_at: now_millis(),
            target_frame: None,
            target_x: 1.0,
            target_y: 2.0,
            text: "x".into(),
        }
    }

    #[test]
    fn records_without_a_held_lock_are_removed() {
        let registry = registry("stale");
        fs::create_dir_all(&registry.dir).unwrap();
        registry.write(&record(99_999)).unwrap();
        assert!(registry.live().is_empty());
        assert!(!registry.json(99_999).exists());
        fs::write(registry.dir.join("123.json"), "not json").unwrap();
        assert!(registry.live().is_empty());
        assert!(!registry.dir.join("123.json").exists());
        let _ = fs::remove_dir_all(&registry.dir);
    }

    #[test]
    fn a_held_lock_is_a_live_arrow() {
        let registry = registry("held");
        let lock = registry.claim(4242).unwrap();
        registry.write(&record(4242)).unwrap();
        assert_eq!(registry.live().len(), 1);
        drop(lock);
        assert!(registry.live().is_empty());
        let _ = fs::remove_dir_all(&registry.dir);
    }
}
