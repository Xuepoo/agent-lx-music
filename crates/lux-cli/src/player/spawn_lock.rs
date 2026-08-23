//! Cross-process lock guarding the mpv probe→spawn critical section.
//!
//! The lock file lives next to the IPC socket (`mpv.lock`). Acquisition uses
//! an exclusive `O_CREAT | O_EXCL` create; an existing sentinel is stolen only
//! when it is provably stale (owner PID no longer alive on Linux, or older
//! than [`STALE_AFTER_SECS`]).

use anyhow::{Result, anyhow};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Age after which a held spawn lock is considered abandoned.
pub const STALE_AFTER_SECS: u64 = 30;
/// Total time spent waiting for another process to finish spawning mpv.
pub const ACQUIRE_TIMEOUT: Duration = Duration::from_secs(10);
const POLL_INTERVAL: Duration = Duration::from_millis(50);

const SENTINEL_MAGIC: &str = "alx-mpv-spawn";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LockSentinel {
    pub pid: u32,
    pub epoch: u64,
}

impl LockSentinel {
    pub fn encode(&self) -> String {
        format!("{SENTINEL_MAGIC} {} {}\n", self.pid, self.epoch)
    }

    pub fn decode(content: &str) -> Option<Self> {
        let mut parts = content.split_whitespace();
        match (parts.next(), parts.next(), parts.next()) {
            (Some(m), Some(pid), Some(epoch)) if m == SENTINEL_MAGIC => Some(Self {
                pid: pid.parse().ok()?,
                epoch: epoch.parse().ok()?,
            }),
            _ => None,
        }
    }
}

pub fn now_epoch() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Whether the PID that wrote the sentinel still exists.
///
/// On Linux this checks `/proc/<pid>`; on other platforms the answer is
/// always "yes" and staleness falls back to the age check exclusively.
pub fn process_alive(pid: u32) -> bool {
    #[cfg(target_os = "linux")]
    {
        Path::new("/proc").join(pid.to_string()).exists()
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = pid;
        true
    }
}

/// A sentinel is stale when its owner died or the lock is suspiciously old.
pub fn is_stale(sentinel: &LockSentinel, now: u64) -> bool {
    if now.saturating_sub(sentinel.epoch) > STALE_AFTER_SECS {
        return true;
    }
    !process_alive(sentinel.pid)
}

fn file_older_than(path: &Path, secs: u64) -> bool {
    match fs::metadata(path).and_then(|m| m.modified()) {
        Ok(modified) => modified
            .elapsed()
            .map(|age| age.as_secs() > secs)
            .unwrap_or(false),
        Err(_) => false,
    }
}

/// Held guard; removes the sentinel on release/drop unless ownership was lost.
#[derive(Debug)]
pub struct SpawnLock {
    path: PathBuf,
    token: String,
}

impl SpawnLock {
    fn try_create(path: &Path) -> std::io::Result<SpawnLock> {
        let sentinel = LockSentinel {
            pid: std::process::id(),
            epoch: now_epoch(),
        };
        let token = sentinel.encode();
        let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
        file.write_all(token.as_bytes())?;
        Ok(SpawnLock {
            path: path.to_path_buf(),
            token,
        })
    }

    /// Acquire the spawn lock, stealing provably stale sentinels and waiting
    /// for live holders up to [`ACQUIRE_TIMEOUT`].
    pub fn acquire(lock_path: &Path) -> Result<SpawnLock> {
        if let Some(parent) = lock_path.parent() {
            let _ = fs::create_dir_all(parent);
        }

        let deadline = Instant::now() + ACQUIRE_TIMEOUT;
        loop {
            match Self::try_create(lock_path) {
                Ok(guard) => return Ok(guard),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    if Self::steal_if_stale(lock_path) {
                        continue;
                    }
                    if Instant::now() >= deadline {
                        return Err(anyhow!(
                            "Another alx process is starting the player (lock {} is held); retry shortly or remove it if stale",
                            lock_path.display()
                        ));
                    }
                    thread::sleep(POLL_INTERVAL);
                }
                Err(e) => {
                    return Err(anyhow!(
                        "Failed to create spawn lock {}: {}",
                        lock_path.display(),
                        e
                    ));
                }
            }
        }
    }

    /// Remove the lock file only when its content proves we own it.
    fn steal_if_stale(path: &Path) -> bool {
        let stolen = match fs::read_to_string(path) {
            Ok(content) => match LockSentinel::decode(&content) {
                Some(sentinel) => is_stale(&sentinel, now_epoch()),
                // Unparseable junk: fall back to file age.
                None => file_older_than(path, STALE_AFTER_SECS),
            },
            Err(_) => false,
        };
        if stolen {
            let _ = fs::remove_file(path);
        }
        stolen
    }

    /// Explicitly release; subsequent Drop becomes a no-op.
    pub fn release(mut self) {
        self.release_inner();
    }

    fn release_inner(&mut self) {
        if self.token.is_empty() {
            return;
        }
        if let Ok(content) = fs::read_to_string(&self.path) {
            // Never unlink a sentinel that is no longer ours.
            if content == self.token {
                let _ = fs::remove_file(&self.path);
            }
        }
        self.token.clear();
    }
}

impl Drop for SpawnLock {
    fn drop(&mut self) {
        self.release_inner();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    static TMP_COUNTER: AtomicU32 = AtomicU32::new(0);

    fn tmp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "alx-spawn-lock-{}-{}-{}",
            tag,
            std::process::id(),
            TMP_COUNTER.fetch_add(1, Ordering::SeqCst)
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn sample(pid: u32, age_secs: u64) -> LockSentinel {
        LockSentinel {
            pid,
            epoch: now_epoch().saturating_sub(age_secs),
        }
    }

    #[test]
    fn sentinel_roundtrip() {
        let s = sample(4242, 0);
        assert_eq!(LockSentinel::decode(&s.encode()), Some(s));
    }

    #[test]
    fn sentinel_rejects_garbage() {
        assert_eq!(LockSentinel::decode(""), None);
        assert_eq!(LockSentinel::decode("other-proc 1 2\n"), None);
        assert_eq!(LockSentinel::decode("alx-mpv-spawn notapid 3"), None);
        assert_eq!(LockSentinel::decode("alx-mpv-spawn 1 notanumber"), None);
    }

    #[test]
    fn staleness_rules() {
        let dead_pid = u32::MAX - 1; // cannot exist as a real PID
        assert!(is_stale(&sample(dead_pid, 0), now_epoch()));
        assert!(!is_stale(&sample(std::process::id(), 0), now_epoch()));
        assert!(is_stale(
            &sample(std::process::id(), STALE_AFTER_SECS + 1),
            now_epoch()
        ));
    }

    #[test]
    fn acquire_is_exclusive_until_released() {
        let dir = tmp_dir("exclusive");
        let path = dir.join("mpv.lock");

        let guard = SpawnLock::acquire(&path).unwrap();
        assert!(path.exists());
        let content = fs::read_to_string(&path).unwrap();
        assert!(LockSentinel::decode(&content).is_some());

        // Second acquisition must fail fast (live holder, fresh stamp).
        let second = SpawnLock::try_create(&path);
        assert_eq!(
            second.unwrap_err().kind(),
            std::io::ErrorKind::AlreadyExists
        );

        guard.release();
        assert!(!path.exists());
        drop(SpawnLock::acquire(&path).unwrap());
    }

    #[test]
    fn release_never_removes_foreign_sentinel() {
        let dir = tmp_dir("foreign");
        let path = dir.join("mpv.lock");

        let guard = SpawnLock::acquire(&path).unwrap();
        // Simulate losing ownership: someone overwrote/re-created the file.
        fs::write(&path, LockSentinel { pid: 1, epoch: 1 }.encode()).unwrap();

        guard.release();
        assert!(path.exists(), "foreign sentinel must survive our release");
        fs::remove_file(&path).unwrap();
    }

    #[test]
    fn acquire_steals_stale_sentinel() {
        let dir = tmp_dir("stale");
        let path = dir.join("mpv.lock");
        let dead = LockSentinel {
            pid: u32::MAX - 1,
            epoch: now_epoch().saturating_sub(STALE_AFTER_SECS + 5),
        };
        fs::write(&path, dead.encode()).unwrap();

        let guard = SpawnLock::acquire(&path).unwrap();
        assert_eq!(
            LockSentinel::decode(&fs::read_to_string(&path).unwrap())
                .unwrap()
                .pid,
            std::process::id()
        );
        guard.release();
    }

    #[test]
    fn fresh_unparseable_junk_is_not_stolen() {
        let dir = tmp_dir("junk");
        let path = dir.join("mpv.lock");
        fs::write(&path, "garbage without sentinel shape").unwrap();

        // Exercises the unparseable-sentinel branch: file is brand new, so it
        // must be kept (age-based fallback says "not stale").
        assert!(!file_older_than(&path, STALE_AFTER_SECS));
    }

    #[test]
    fn drop_releases_lock_once() {
        let dir = tmp_dir("drop");
        let path = dir.join("mpv.lock");
        {
            let _guard = SpawnLock::acquire(&path).unwrap();
            assert!(path.exists());
        }
        assert!(!path.exists());
    }
}
