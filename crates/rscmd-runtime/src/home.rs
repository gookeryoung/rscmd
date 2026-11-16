//! 配置、状态与缓存根目录解析（`RSCMD_HOME`）。
//!
//! 优先级：`RSCMD_HOME` > 平台默认目录；解析依赖全部可注入，便于覆盖跨平台分支。
//! 本模块只计算路径，不创建目录，也不做任何 IO。

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

/// 平台类别；测试可注入，不依赖编译目标。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    Windows,
    MacOs,
    Unix,
}

impl Platform {
    pub fn current() -> Self {
        if cfg!(windows) {
            Self::Windows
        } else if cfg!(target_os = "macos") {
            Self::MacOs
        } else {
            Self::Unix
        }
    }
}

/// 目录解析所依赖的环境：变量表、平台与主目录都可注入。
#[derive(Debug, Clone)]
pub struct DirEnv {
    pub vars: BTreeMap<String, String>,
    pub platform: Platform,
    pub home_dir: Option<PathBuf>,
}

impl DirEnv {
    /// 读取真实进程环境；非 UTF-8 变量直接忽略，避免 `env::vars` 在异常环境下 panic。
    pub fn from_process() -> Self {
        let vars = std::env::vars_os()
            .filter_map(|(key, value)| Some((key.into_string().ok()?, value.into_string().ok()?)))
            .collect();
        Self {
            vars,
            platform: Platform::current(),
            home_dir: std::env::var_os("HOME")
                .or_else(|| std::env::var_os("USERPROFILE"))
                .map(PathBuf::from),
        }
    }

    fn var(&self, key: &str) -> Option<&Path> {
        self.vars
            .get(key)
            .map(String::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(Path::new)
    }

    fn home(&self) -> Option<&Path> {
        self.home_dir
            .as_deref()
            .filter(|path| !path.as_os_str().is_empty())
            .or_else(|| self.var("HOME"))
            .or_else(|| self.var("USERPROFILE"))
    }
}

/// 已解析的目录根。子目录按需拼接，不触发 IO。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dirs {
    root: PathBuf,
}

impl Dirs {
    /// 解析根目录；没有可用来源时返回错误（调用方自行决定是否降级）。
    pub fn resolve(env: &DirEnv) -> Result<Self, DirError> {
        if let Some(explicit) = env.var("RSCMD_HOME") {
            return Ok(Self {
                root: explicit.to_path_buf(),
            });
        }
        let root = match env.platform {
            Platform::Windows => env.var("LOCALAPPDATA").map(Path::to_path_buf),
            Platform::MacOs => env
                .home()
                .map(|home| home.join("Library").join("Application Support")),
            Platform::Unix => env
                .var("XDG_DATA_HOME")
                .map(Path::to_path_buf)
                .or_else(|| env.home().map(|home| home.join(".local").join("share"))),
        }
        .ok_or_else(|| DirError::new("无法确定 RSCMD_HOME、平台数据目录或用户主目录"))?;
        Ok(Self {
            root: root.join("rscmd"),
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// 下载缓存目录：`<root>/cache/downloads`。
    pub fn cache_dir(&self) -> PathBuf {
        self.root.join("cache").join("downloads")
    }

    pub fn state_dir(&self) -> PathBuf {
        self.root.join("state")
    }

    pub fn logs_dir(&self) -> PathBuf {
        self.root.join("logs")
    }
}

/// 目录解析失败；缓存层会把它当作「未启用」静默降级。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirError {
    message: String,
}

impl DirError {
    fn new(message: &str) -> Self {
        Self {
            message: message.to_string(),
        }
    }
}

impl fmt::Display for DirError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for DirError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(platform: Platform, pairs: &[(&str, &str)]) -> DirEnv {
        DirEnv {
            vars: pairs
                .iter()
                .map(|(key, value)| (key.to_string(), value.to_string()))
                .collect(),
            platform,
            home_dir: None,
        }
    }

    #[test]
    fn rscmd_home_wins_on_every_platform() {
        for platform in [Platform::Windows, Platform::MacOs, Platform::Unix] {
            let dirs = Dirs::resolve(&env(
                platform,
                &[
                    ("RSCMD_HOME", "D:/custom/rscmd-home"),
                    ("LOCALAPPDATA", "C:/Users/me/AppData/Local"),
                    ("HOME", "/home/me"),
                    ("XDG_DATA_HOME", "/home/me/.local/share"),
                ],
            ))
            .expect("显式 RSCMD_HOME 应生效");
            assert_eq!(dirs.root(), Path::new("D:/custom/rscmd-home"));
            assert_eq!(
                dirs.cache_dir(),
                Path::new("D:/custom/rscmd-home")
                    .join("cache")
                    .join("downloads")
            );
            assert_eq!(
                dirs.state_dir(),
                Path::new("D:/custom/rscmd-home").join("state")
            );
            assert_eq!(
                dirs.logs_dir(),
                Path::new("D:/custom/rscmd-home").join("logs")
            );
        }
    }

    #[test]
    fn blank_rscmd_home_falls_back_to_platform_default() {
        let dirs = Dirs::resolve(&env(
            Platform::Windows,
            &[
                ("RSCMD_HOME", "  "),
                ("LOCALAPPDATA", "C:/Users/me/AppData/Local"),
            ],
        ))
        .unwrap();
        assert_eq!(
            dirs.root(),
            Path::new("C:/Users/me/AppData/Local").join("rscmd")
        );
    }

    #[test]
    fn windows_uses_local_app_data() {
        let dirs = Dirs::resolve(&env(
            Platform::Windows,
            &[("LOCALAPPDATA", "C:/Users/me/AppData/Local")],
        ))
        .unwrap();
        assert_eq!(
            dirs.root(),
            Path::new("C:/Users/me/AppData/Local").join("rscmd")
        );
    }

    #[test]
    fn macos_uses_application_support() {
        let dirs = Dirs::resolve(&env(Platform::MacOs, &[("HOME", "/Users/me")])).unwrap();
        assert_eq!(
            dirs.root(),
            Path::new("/Users/me")
                .join("Library")
                .join("Application Support")
                .join("rscmd")
        );
    }

    #[test]
    fn unix_prefers_xdg_then_home_fallback() {
        let dirs = Dirs::resolve(&env(
            Platform::Unix,
            &[("XDG_DATA_HOME", "/data"), ("HOME", "/home/me")],
        ))
        .unwrap();
        assert_eq!(dirs.root(), Path::new("/data").join("rscmd"));

        let dirs = Dirs::resolve(&env(Platform::Unix, &[("HOME", "/home/me")])).unwrap();
        assert_eq!(
            dirs.root(),
            Path::new("/home/me")
                .join(".local")
                .join("share")
                .join("rscmd")
        );
    }

    #[test]
    fn injected_home_dir_is_used_when_vars_are_missing() {
        let mut env = env(Platform::Unix, &[]);
        env.home_dir = Some(PathBuf::from("/injected"));
        let dirs = Dirs::resolve(&env).unwrap();
        assert_eq!(
            dirs.root(),
            Path::new("/injected")
                .join(".local")
                .join("share")
                .join("rscmd")
        );
    }

    #[test]
    fn missing_all_sources_is_an_error() {
        let err = Dirs::resolve(&env(Platform::Unix, &[])).unwrap_err();
        assert!(err.to_string().contains("无法确定"));
        assert_eq!(
            err,
            DirError::new("无法确定 RSCMD_HOME、平台数据目录或用户主目录")
        );
    }

    #[test]
    fn platform_current_matches_build_target() {
        let expected = if cfg!(windows) {
            Platform::Windows
        } else if cfg!(target_os = "macos") {
            Platform::MacOs
        } else {
            Platform::Unix
        };
        assert_eq!(Platform::current(), expected);
    }

    #[test]
    fn from_process_reads_without_panicking() {
        let env = DirEnv::from_process();
        // 真实环境至少有平台与变量表；解析成功与否都允许，但不能 panic。
        let _ = Dirs::resolve(&env);
        assert_eq!(env.platform, Platform::current());
    }
}
