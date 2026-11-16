//! 下载缓存：按 URL 内容寻址、TTL 失效、容量淘汰。
//!
//! 所有失败都按「未命中」或「本次不写缓存」处理，不向调用方报错；
//! 缓存不可用等同于未启用，网络下载路径不受影响。

use std::fs;
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::fs::atomic_write;
use crate::home::Dirs;
use crate::net::sha256_hex;

const CACHE_SCHEMA: &str = "rscmd-cache/1";

/// 缓存失效与容量边界。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CachePolicy {
    /// 条目存活时长，按 `fetched_at` 惰性判定。
    pub ttl: Duration,
    /// 缓存目录内容总大小上限（字节）。
    pub max_bytes: u64,
    /// 条目数量上限。
    pub max_entries: usize,
}

impl Default for CachePolicy {
    fn default() -> Self {
        Self {
            ttl: Duration::from_secs(7 * 24 * 60 * 60),
            max_bytes: 256 * 1024 * 1024,
            max_entries: 1024,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct Meta {
    schema: String,
    url: String,
    size: u64,
    sha256: String,
    fetched_at: u64,
}

/// 下载缓存句柄。目录不可创建时 [`DownloadCache::open`] 返回 `None`。
pub struct DownloadCache {
    root: PathBuf,
    policy: CachePolicy,
    clock: Box<dyn Fn() -> SystemTime>,
}

impl DownloadCache {
    /// 打开（必要时创建）缓存目录；任何失败都返回 `None`，调用方继续走网络。
    pub fn open(dirs: &Dirs, policy: CachePolicy) -> Option<Self> {
        let root = dirs.cache_dir();
        if fs::create_dir_all(&root).is_err() {
            return None;
        }
        Some(Self::with_clock(root, policy, Box::new(SystemTime::now)))
    }

    /// 指定根目录与时钟，供测试注入；不创建目录。
    pub fn with_clock(
        root: PathBuf,
        policy: CachePolicy,
        clock: Box<dyn Fn() -> SystemTime>,
    ) -> Self {
        Self {
            root,
            policy,
            clock,
        }
    }

    /// 读取命中项；过期、损坏或缺失都返回 `None`，并顺手删除坏条目。
    pub fn get(&self, url: &str) -> Option<Vec<u8>> {
        let key = cache_key(url);
        let meta = self.read_meta(&key)?;
        if meta.schema != CACHE_SCHEMA || meta.url != normalize_url(url) || self.expired(&meta) {
            self.remove(&key);
            return None;
        }
        let data = match fs::read(self.data_path(&key)) {
            Ok(data) => data,
            Err(_) => {
                self.remove(&key);
                return None;
            }
        };
        if data.len() as u64 != meta.size || sha256_hex(&data) != meta.sha256 {
            self.remove(&key);
            return None;
        }
        Some(data)
    }

    /// 写入缓存；任何 IO 失败都静默放弃本次写入。
    pub fn put(&self, url: &str, data: &[u8]) {
        let key = cache_key(url);
        let meta = Meta {
            schema: CACHE_SCHEMA.to_string(),
            url: normalize_url(url),
            size: data.len() as u64,
            sha256: sha256_hex(data),
            fetched_at: self.now_secs(),
        };
        let Ok(meta_bytes) = serde_json::to_vec(&meta) else {
            return;
        };
        // Windows 的 rename 不能覆盖已有文件；缓存允许短暂缺失，先删旧文件。
        let data_path = self.data_path(&key);
        let _ = fs::remove_file(&data_path);
        if atomic_write(&data_path, data).is_err() {
            return;
        }
        let meta_path = self.meta_path(&key);
        if atomic_write(&meta_path, &meta_bytes).is_err() {
            let _ = fs::remove_file(&data_path);
            return;
        }
        self.cleanup();
    }

    fn read_meta(&self, key: &str) -> Option<Meta> {
        let raw = match fs::read(self.meta_path(key)) {
            Ok(raw) => raw,
            Err(_) => {
                self.remove(key);
                return None;
            }
        };
        match serde_json::from_slice(&raw) {
            Ok(meta) => Some(meta),
            Err(_) => {
                self.remove(key);
                None
            }
        }
    }

    fn expired(&self, meta: &Meta) -> bool {
        self.now_secs().saturating_sub(meta.fetched_at) > self.policy.ttl.as_secs()
    }

    fn now_secs(&self) -> u64 {
        (self.clock)()
            .duration_since(UNIX_EPOCH)
            .map(|elapsed| elapsed.as_secs())
            .unwrap_or(0)
    }

    fn remove(&self, key: &str) {
        let _ = fs::remove_file(self.meta_path(key));
        let _ = fs::remove_file(self.data_path(key));
    }

    fn data_path(&self, key: &str) -> PathBuf {
        self.root.join(format!("{key}.bin"))
    }

    fn meta_path(&self, key: &str) -> PathBuf {
        self.root.join(format!("{key}.json"))
    }

    /// 清理临时文件、孤立数据文件，并按容量上限从旧到新淘汰。
    fn cleanup(&self) {
        let Ok(entries) = fs::read_dir(&self.root) else {
            return;
        };
        let mut metas: Vec<(u64, u64, PathBuf)> = Vec::new();
        let mut total: u64 = 0;
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.contains(".rscmd-tmp-") {
                let _ = fs::remove_file(&path);
                continue;
            }
            if !name.ends_with(".json") {
                continue;
            }
            let Ok(raw) = fs::read(&path) else {
                let _ = fs::remove_file(&path);
                continue;
            };
            let Ok(meta) = serde_json::from_slice::<Meta>(&raw) else {
                let _ = fs::remove_file(&path);
                continue;
            };
            let data = path.with_extension("bin");
            if !data.exists() {
                let _ = fs::remove_file(&path);
                continue;
            }
            total += meta.size;
            metas.push((meta.fetched_at, meta.size, path));
        }

        // 孤立 .bin（没有同名 .json）没有用处，直接删除。
        if let Ok(entries) = fs::read_dir(&self.root) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|ext| ext.to_str()) != Some("bin") {
                    continue;
                }
                if !path.with_extension("json").exists() {
                    let _ = fs::remove_file(&path);
                }
            }
        }

        metas.sort_by_key(|(fetched_at, _, _)| *fetched_at);
        let mut count = metas.len();
        while (total > self.policy.max_bytes || count > self.policy.max_entries)
            && !metas.is_empty()
        {
            let (_, size, meta_path) = metas.remove(0);
            let _ = fs::remove_file(&meta_path);
            let _ = fs::remove_file(meta_path.with_extension("bin"));
            total = total.saturating_sub(size);
            count -= 1;
        }
    }
}

/// 缓存键：规范化 URL 的 SHA-256 小写 hex。
pub fn cache_key(url: &str) -> String {
    sha256_hex(normalize_url(url).as_bytes())
}

/// URL 规范化：去 fragment，scheme 与 authority 小写；path/query 保持原样。
pub fn normalize_url(url: &str) -> String {
    let trimmed = url.trim();
    let without_fragment = trimmed.split('#').next().unwrap_or(trimmed);
    let Some((scheme, rest)) = without_fragment.split_once("://") else {
        return without_fragment.to_string();
    };
    let end = rest.find(['/', '?']).unwrap_or(rest.len());
    let (authority, tail) = rest.split_at(end);
    format!(
        "{}://{}{}",
        scheme.to_ascii_lowercase(),
        authority.to_ascii_lowercase(),
        tail
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs::temp_dir;
    use std::cell::Cell;
    use std::path::Path;
    use std::rc::Rc;

    /// 供测试固定时钟的最小工具：`Rc<Cell<_>>` 可读且可推进。
    fn fixed_clock(seconds: u64) -> (Rc<Cell<u64>>, Box<dyn Fn() -> SystemTime>) {
        let holder = Rc::new(Cell::new(seconds));
        let reader = Rc::clone(&holder);
        (
            holder,
            Box::new(move || UNIX_EPOCH + Duration::from_secs(reader.get())),
        )
    }

    fn cache(dir: &Path, policy: CachePolicy, seconds: u64) -> (Rc<Cell<u64>>, DownloadCache) {
        let (clock, reader) = fixed_clock(seconds);
        (
            clock,
            DownloadCache::with_clock(dir.to_path_buf(), policy, reader),
        )
    }

    #[test]
    fn cache_hit_returns_payload() {
        let dir = temp_dir().unwrap();
        let (_, cache) = cache(dir.path(), CachePolicy::default(), 1_000);
        cache.put("https://example.com/a", b"hello");
        assert_eq!(cache.get("https://example.com/a").unwrap(), b"hello");
        // 未写入的 URL 未命中。
        assert!(cache.get("https://example.com/b").is_none());
    }

    #[test]
    fn ttl_expires_and_removes_entry() {
        let dir = temp_dir().unwrap();
        let policy = CachePolicy {
            ttl: Duration::from_secs(60),
            ..CachePolicy::default()
        };
        let (clock, cache) = cache(dir.path(), policy, 1_000);
        cache.put("https://example.com/a", b"hello");
        assert!(cache.get("https://example.com/a").is_some());

        clock.set(1_061);
        assert!(cache.get("https://example.com/a").is_none());
        let key = cache_key("https://example.com/a");
        assert!(!dir.path().join(format!("{key}.bin")).exists());
        assert!(!dir.path().join(format!("{key}.json")).exists());
    }

    #[test]
    fn corrupted_payload_is_discarded() {
        let dir = temp_dir().unwrap();
        let (_, cache) = cache(dir.path(), CachePolicy::default(), 1_000);
        cache.put("https://example.com/a", b"hello");
        let key = cache_key("https://example.com/a");
        fs::write(dir.path().join(format!("{key}.bin")), b"tampered").unwrap();
        assert!(cache.get("https://example.com/a").is_none());
        assert!(!dir.path().join(format!("{key}.bin")).exists());
    }

    #[test]
    fn corrupted_meta_is_discarded() {
        let dir = temp_dir().unwrap();
        let (_, cache) = cache(dir.path(), CachePolicy::default(), 1_000);
        cache.put("https://example.com/a", b"hello");
        let key = cache_key("https://example.com/a");
        fs::write(dir.path().join(format!("{key}.json")), b"not json").unwrap();
        assert!(cache.get("https://example.com/a").is_none());
        assert!(!dir.path().join(format!("{key}.json")).exists());
    }

    #[test]
    fn capacity_evicts_oldest_until_limits_hold() {
        let dir = temp_dir().unwrap();
        let policy = CachePolicy {
            ttl: Duration::from_secs(1000),
            max_bytes: 30,
            max_entries: 10,
        };
        let (clock, cache) = cache(dir.path(), policy, 1_000);
        cache.put("https://example.com/old", b"0123456789");
        clock.set(1_010);
        cache.put("https://example.com/mid", b"0123456789");
        clock.set(1_020);
        cache.put("https://example.com/new", b"0123456789");

        // 总大小 30 字节不超限；再加一条会触发按 fetched_at 淘汰最旧的。
        clock.set(1_030);
        cache.put("https://example.com/newest", b"0123456789");
        assert!(cache.get("https://example.com/old").is_none());
        assert!(cache.get("https://example.com/newest").is_some());
    }

    #[test]
    fn entry_count_limit_is_enforced() {
        let dir = temp_dir().unwrap();
        let policy = CachePolicy {
            ttl: Duration::from_secs(1000),
            max_bytes: u64::MAX,
            max_entries: 1,
        };
        let (clock, cache) = cache(dir.path(), policy, 1_000);
        cache.put("https://example.com/a", b"a");
        clock.set(1_001);
        cache.put("https://example.com/b", b"b");
        assert!(cache.get("https://example.com/a").is_none());
        assert_eq!(cache.get("https://example.com/b").unwrap(), b"b");
    }

    #[test]
    fn temp_and_orphan_files_are_cleaned() {
        let dir = temp_dir().unwrap();
        let (_, cache) = cache(dir.path(), CachePolicy::default(), 1_000);
        fs::write(dir.path().join("stale.rscmd-tmp-abc"), b"x").unwrap();
        fs::write(dir.path().join("orphan.bin"), b"y").unwrap();
        cache.put("https://example.com/a", b"hello");
        assert!(!dir.path().join("stale.rscmd-tmp-abc").exists());
        assert!(!dir.path().join("orphan.bin").exists());
        assert!(cache.get("https://example.com/a").is_some());
    }

    #[test]
    fn unusable_root_degrades_silently() {
        let dir = temp_dir().unwrap();
        // 用一个普通文件占据缓存目录路径，open 必须返回 None。
        let blocked = dir.path().join("blocked");
        fs::write(&blocked, b"file").unwrap();
        let dirs = crate::home::Dirs::resolve(&crate::home::DirEnv {
            vars: [(
                "RSCMD_HOME".to_string(),
                blocked.to_string_lossy().into_owned(),
            )]
            .into_iter()
            .collect(),
            platform: crate::home::Platform::Unix,
            home_dir: None,
        })
        .unwrap();
        assert!(DownloadCache::open(&dirs, CachePolicy::default()).is_none());

        // 即便根目录不可用，显式构造的缓存写/读也只返回未命中，不 panic。
        let cache = DownloadCache::with_clock(
            blocked.clone(),
            CachePolicy::default(),
            Box::new(SystemTime::now),
        );
        cache.put("https://example.com/a", b"hello");
        assert!(cache.get("https://example.com/a").is_none());
    }

    #[test]
    fn normalize_url_lowercases_scheme_and_host_only() {
        assert_eq!(
            normalize_url(" HTTP://Example.COM/Path?Q=A#frag "),
            "http://example.com/Path?Q=A"
        );
        assert_eq!(
            normalize_url("https://Example.COM?Q=A"),
            "https://example.com?Q=A"
        );
        assert_eq!(normalize_url("not-a-url"), "not-a-url");
        assert_eq!(
            cache_key("HTTP://Example.COM/a#one"),
            cache_key("http://example.com/a#two")
        );
        assert_ne!(
            cache_key("https://example.com/a"),
            cache_key("https://example.com/b")
        );
    }

    #[test]
    fn default_policy_has_bounded_limits() {
        let policy = CachePolicy::default();
        assert_eq!(policy.ttl, Duration::from_secs(7 * 24 * 60 * 60));
        assert_eq!(policy.max_bytes, 256 * 1024 * 1024);
        assert_eq!(policy.max_entries, 1024);
    }
}
