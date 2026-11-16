//! 跨进程目录锁：用 `create_new` 独占创建锁文件。
//!
//! 锁只在同一路径上互斥，不跨目录；文件内容仅为排障信息（pid 与获取时间），
//! 不参与判定。`DirLock` 在 Drop 时删除锁文件，正常返回与 unwind 都能释放；
//! 进程被强杀时锁文件会残留，因此调用方应使用带超时的 [`DirLock::acquire`]，
//! 不要无限等待。

use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// 轮询间隔：短等待足够灵敏，长等待也不会占满 CPU。
const POLL_INTERVAL: Duration = Duration::from_millis(10);

/// 锁获取失败；`code` 与 `rscmd-core` 的 `ERROR_CODE_KINDS` 登记一致。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LockError {
    /// 锁已被其他进程（或同一进程的其他句柄）持有。
    Busy(PathBuf),
    /// 等待超过给定上限仍未拿到锁。
    Timeout { path: PathBuf, waited: Duration },
    /// 锁文件无法创建：目录不存在、只读或权限不足。
    Unavailable { path: PathBuf, reason: String },
}

impl LockError {
    /// 统一错误码：`LOCK_BUSY` / `LOCK_TIMEOUT` / `LOCK_UNAVAILABLE`。
    pub fn code(&self) -> &'static str {
        match self {
            Self::Busy(_) => "LOCK_BUSY",
            Self::Timeout { .. } => "LOCK_TIMEOUT",
            Self::Unavailable { .. } => "LOCK_UNAVAILABLE",
        }
    }

    pub fn path(&self) -> &Path {
        match self {
            Self::Busy(path) => path,
            Self::Timeout { path, .. } => path,
            Self::Unavailable { path, .. } => path,
        }
    }
}

impl fmt::Display for LockError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Busy(path) => write!(formatter, "锁文件 {} 已被其他进程持有", path.display()),
            Self::Timeout { path, waited } => write!(
                formatter,
                "等待锁文件 {} 超时（已等待 {} ms）",
                path.display(),
                waited.as_millis()
            ),
            Self::Unavailable { path, reason } => {
                write!(formatter, "无法创建锁文件 {}: {reason}", path.display())
            }
        }
    }
}

impl std::error::Error for LockError {}

/// 已持有的目录锁；Drop 时删除锁文件。
#[derive(Debug)]
pub struct DirLock {
    path: PathBuf,
}

impl DirLock {
    /// 立即尝试获取；已被占用时返回 [`LockError::Busy`]，不等待。
    pub fn try_acquire(dir: &Path, name: &str) -> Result<Self, LockError> {
        let path = lock_path(dir, name)?;
        create(&path)?;
        Ok(Self { path })
    }

    /// 在 `timeout` 内轮询等待；超时返回 [`LockError::Timeout`]，不创建锁文件。
    pub fn acquire(dir: &Path, name: &str, timeout: Duration) -> Result<Self, LockError> {
        let path = lock_path(dir, name)?;
        let deadline = Instant::now() + timeout;
        loop {
            match create(&path) {
                Ok(()) => return Ok(Self { path }),
                Err(LockError::Busy(_)) => {
                    let now = Instant::now();
                    if now >= deadline {
                        return Err(LockError::Timeout {
                            path,
                            waited: timeout,
                        });
                    }
                    thread::sleep(POLL_INTERVAL.min(deadline - now));
                }
                Err(err) => return Err(err),
            }
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for DirLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

/// 锁名只允许作为文件名使用，含路径分隔符或为空时直接拒绝，避免逃逸出目标目录。
fn lock_path(dir: &Path, name: &str) -> Result<PathBuf, LockError> {
    let path = dir.join(format!("{name}.lock"));
    if name.is_empty() || name.contains(['/', '\\']) || name == "." || name == ".." {
        return Err(LockError::Unavailable {
            path,
            reason: "锁名不能为空或包含路径分隔符".to_string(),
        });
    }
    Ok(path)
}

fn create(path: &Path) -> Result<(), LockError> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent).map_err(|err| LockError::Unavailable {
            path: path.to_path_buf(),
            reason: err.to_string(),
        })?;
    }
    match OpenOptions::new().write(true).create_new(true).open(path) {
        Ok(mut file) => {
            // 内容只用于排障，写入失败不影响互斥语义。
            let _ = file.write_all(
                format!("pid={}\nfetched_at={}\n", std::process::id(), epoch_secs()).as_bytes(),
            );
            Ok(())
        }
        Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
            Err(LockError::Busy(path.to_path_buf()))
        }
        Err(err) => Err(LockError::Unavailable {
            path: path.to_path_buf(),
            reason: err.to_string(),
        }),
    }
}

fn epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs::temp_dir;

    #[test]
    fn acquire_then_release_removes_lock_file() {
        let dir = temp_dir().unwrap();
        let lock = DirLock::try_acquire(dir.path(), "key").unwrap();
        let path = lock.path().to_path_buf();
        assert!(path.exists());
        drop(lock);
        assert!(!path.exists());
        // 释放后可以被再次获取。
        DirLock::try_acquire(dir.path(), "key").unwrap();
    }

    #[test]
    fn second_try_reports_busy() {
        let dir = temp_dir().unwrap();
        let held = DirLock::try_acquire(dir.path(), "key").unwrap();
        let err = DirLock::try_acquire(dir.path(), "key").unwrap_err();
        assert_eq!(err, LockError::Busy(held.path().to_path_buf()));
        assert_eq!(err.code(), "LOCK_BUSY");
        assert!(err.to_string().contains("已被其他进程持有"));
        assert_eq!(err.path(), held.path());
    }

    #[test]
    fn acquire_waits_until_released() {
        let dir = temp_dir().unwrap();
        let held = DirLock::try_acquire(dir.path(), "key").unwrap();
        let handle = thread::spawn(move || {
            thread::sleep(Duration::from_millis(120));
            drop(held);
        });
        let start = Instant::now();
        let lock = DirLock::acquire(dir.path(), "key", Duration::from_secs(10)).unwrap();
        assert!(start.elapsed() >= Duration::from_millis(100));
        drop(lock);
        handle.join().unwrap();
    }

    #[test]
    fn acquire_times_out_while_held() {
        let dir = temp_dir().unwrap();
        let held = DirLock::try_acquire(dir.path(), "key").unwrap();
        let err = DirLock::acquire(dir.path(), "key", Duration::from_millis(30)).unwrap_err();
        assert_eq!(
            err,
            LockError::Timeout {
                path: held.path().to_path_buf(),
                waited: Duration::from_millis(30),
            }
        );
        assert_eq!(err.code(), "LOCK_TIMEOUT");
        assert!(err.to_string().contains("超时"));
    }

    #[test]
    fn panic_releases_lock_file() {
        let dir = temp_dir().unwrap();
        let path = dir.path().join("key.lock");
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _lock = DirLock::try_acquire(dir.path(), "key").unwrap();
            assert!(path.exists());
            panic!("模拟工具 panic");
        }));
        assert!(result.is_err());
        assert!(!path.exists());
    }

    #[test]
    fn unusable_dir_reports_unavailable() {
        let dir = temp_dir().unwrap();
        // 用普通文件占据目录位置：创建父目录必然失败。
        let blocked = dir.path().join("blocked");
        fs::write(&blocked, b"file").unwrap();
        let err = DirLock::try_acquire(&blocked, "key").unwrap_err();
        assert!(matches!(err, LockError::Unavailable { .. }));
        assert_eq!(err.code(), "LOCK_UNAVAILABLE");
        assert!(err.path().ends_with("key.lock"));
        assert!(!err.to_string().contains("已被其他进程持有"));
    }

    #[test]
    fn invalid_names_are_rejected() {
        let dir = temp_dir().unwrap();
        for name in ["", ".", "..", "a/b", "a\\b"] {
            let err = DirLock::try_acquire(dir.path(), name).unwrap_err();
            assert_eq!(err.code(), "LOCK_UNAVAILABLE", "锁名 {name:?} 应被拒绝");
            assert!(err.to_string().contains("锁名"));
        }
        // 合法名字能正常创建，且锁文件位于目标目录内。
        let lock = DirLock::try_acquire(dir.path(), "ok").unwrap();
        assert_eq!(lock.path(), dir.path().join("ok.lock"));
    }
}
