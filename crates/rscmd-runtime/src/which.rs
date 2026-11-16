//! 可执行文件查找：PATH 顺序、Windows PATHEXT、Unix 可执行位。
//!
//! 依赖（PATH、PATHEXT、平台、cwd）全部可注入，便于在任一宿主上覆盖各平台分支。

use std::path::{Path, PathBuf};

use crate::fs::is_absolute_path;
use crate::home::Platform;

/// 查找所依赖的环境快照。
#[derive(Debug, Clone)]
pub struct WhichEnv {
    pub path_var: String,
    pub pathext: String,
    pub platform: Platform,
    pub cwd: PathBuf,
}

impl WhichEnv {
    /// 读取真实进程环境；PATH 缺失时按空处理，PATHEXT 缺失时用 Windows 默认值。
    pub fn from_process() -> Self {
        Self {
            path_var: std::env::var("PATH").unwrap_or_default(),
            pathext: std::env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".to_string()),
            platform: Platform::current(),
            cwd: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
        }
    }
}

/// 在 PATH 中查找可执行文件，返回首个命中的绝对路径；找不到返回 `None`。
///
/// - 含 `/` 或 `\` 的 `program` 按直接路径处理（相对路径相对 `cwd`）；
/// - Windows：无扩展名时依次尝试 PATHEXT，只要求是普通文件；
/// - Unix：要求普通文件且有任一可执行位（非 Unix 宿主无法读取权限时退化为存在性判断）。
pub fn find(program: &str, env: &WhichEnv) -> Option<PathBuf> {
    if program.is_empty() {
        return None;
    }
    if program.contains('/') || program.contains('\\') {
        let candidate = resolve(program, env);
        return is_executable(&candidate, env.platform).then_some(candidate);
    }

    let names = candidate_names(program, env);
    let separator = if env.platform == Platform::Windows {
        ';'
    } else {
        ':'
    };
    for dir in env.path_var.split(separator) {
        let dir_path = if dir.is_empty() {
            // Unix 空段表示当前目录；Windows 空段通常来自尾随分号，直接跳过。
            if env.platform == Platform::Windows {
                continue;
            }
            env.cwd.clone()
        } else {
            resolve(dir, env)
        };
        for name in &names {
            let candidate = dir_path.join(name);
            if is_executable(&candidate, env.platform) {
                return Some(candidate);
            }
        }
    }
    None
}

/// 按平台规则展开候选文件名（Windows 追加 PATHEXT）。
fn candidate_names(program: &str, env: &WhichEnv) -> Vec<String> {
    let mut names = vec![program.to_string()];
    if env.platform == Platform::Windows && Path::new(program).extension().is_none() {
        for ext in env.pathext.split(';').filter(|ext| !ext.is_empty()) {
            let suffix = if ext.starts_with('.') {
                ext.to_string()
            } else {
                format!(".{ext}")
            };
            names.push(format!("{program}{suffix}"));
        }
    }
    names
}

fn resolve(raw: &str, env: &WhichEnv) -> PathBuf {
    if is_absolute_path(raw) {
        PathBuf::from(raw)
    } else {
        env.cwd.join(raw)
    }
}

fn is_executable(path: &Path, platform: Platform) -> bool {
    let Ok(metadata) = std::fs::metadata(path) else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    match platform {
        Platform::Windows => true,
        Platform::MacOs | Platform::Unix => has_exec_bit(&metadata),
    }
}

#[cfg(unix)]
fn has_exec_bit(metadata: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    metadata.permissions().mode() & 0o111 != 0
}

/// 非 Unix 宿主读不到可执行位，退化为「存在即可执行」，避免跨平台测试被宿主限制。
#[cfg(not(unix))]
fn has_exec_bit(_metadata: &std::fs::Metadata) -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs::temp_dir;

    fn env(platform: Platform, path_var: String, cwd: PathBuf) -> WhichEnv {
        WhichEnv {
            path_var,
            pathext: ".COM;.EXE;.BAT;.CMD".to_string(),
            platform,
            cwd,
        }
    }

    fn write_file(dir: &Path, name: &str, executable: bool) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, b"x").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = if executable { 0o755 } else { 0o644 };
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
        }
        let _ = executable;
        path
    }

    #[test]
    fn finds_first_match_in_path_order() {
        let dir = temp_dir().unwrap();
        let first = dir.path().join("a");
        let second = dir.path().join("b");
        std::fs::create_dir_all(&first).unwrap();
        std::fs::create_dir_all(&second).unwrap();
        write_file(&first, "tool", true);
        write_file(&second, "tool", true);
        // 用相对目录构造 PATH：Unix 分隔符是 ':'，Windows 盘符里的 ':' 会干扰解析。
        let found = find(
            "tool",
            &env(Platform::Unix, "a:b".to_string(), dir.path().to_path_buf()),
        )
        .unwrap();
        assert_eq!(found, first.join("tool"));
    }

    #[test]
    fn returns_none_when_missing() {
        let dir = temp_dir().unwrap();
        let env = env(Platform::Unix, String::new(), dir.path().to_path_buf());
        assert!(find("definitely-not-here", &env).is_none());
    }

    #[test]
    fn windows_tries_pathext_variants() {
        let dir = temp_dir().unwrap();
        write_file(dir.path(), "tool.exe", false);
        let mut env = env(
            Platform::Windows,
            dir.path().display().to_string(),
            dir.path().to_path_buf(),
        );
        // 用小写 PATHEXT，让用例在大小写敏感的 Unix 宿主上也能命中同一文件。
        env.pathext = ".exe".to_string();
        let found = find("tool", &env).unwrap();
        assert_eq!(found, dir.path().join("tool.exe"));
    }

    #[test]
    fn windows_keeps_explicit_extension() {
        let dir = temp_dir().unwrap();
        write_file(dir.path(), "tool.bat", false);
        let env = env(
            Platform::Windows,
            dir.path().display().to_string(),
            dir.path().to_path_buf(),
        );
        assert_eq!(find("tool.bat", &env).unwrap(), dir.path().join("tool.bat"));
    }

    #[test]
    fn windows_skips_empty_path_segments() {
        let dir = temp_dir().unwrap();
        write_file(dir.path(), "only.exe", false);
        let mut env = env(
            Platform::Windows,
            format!(";{};", dir.path().display()),
            dir.path().to_path_buf(),
        );
        env.pathext = ".exe".to_string();
        assert!(find("only", &env).is_some());
    }

    #[test]
    fn unix_empty_segment_means_cwd() {
        let dir = temp_dir().unwrap();
        write_file(dir.path(), "local", true);
        let env = env(Platform::Unix, String::new(), dir.path().to_path_buf());
        let found = find("local", &env).unwrap();
        assert_eq!(found, dir.path().join("local"));
    }

    #[cfg(unix)]
    #[test]
    fn unix_requires_exec_bit() {
        let dir = temp_dir().unwrap();
        write_file(dir.path(), "plain", false);
        let env = env(
            Platform::Unix,
            dir.path().display().to_string(),
            dir.path().to_path_buf(),
        );
        assert!(find("plain", &env).is_none(), "无可执行位不应命中");
    }

    #[test]
    fn program_with_separator_is_resolved_against_cwd() {
        let dir = temp_dir().unwrap();
        std::fs::create_dir_all(dir.path().join("bin")).unwrap();
        write_file(&dir.path().join("bin"), "tool", true);
        let env = env(Platform::Unix, String::new(), dir.path().to_path_buf());
        let found = find("bin/tool", &env).unwrap();
        assert_eq!(found, dir.path().join("bin").join("tool"));
    }

    #[test]
    fn absolute_program_is_used_as_is() {
        let dir = temp_dir().unwrap();
        let path = write_file(dir.path(), "abs", true);
        let env = env(Platform::Unix, String::new(), PathBuf::from("C:/elsewhere"));
        assert_eq!(find(&path.display().to_string(), &env), Some(path));
    }

    #[test]
    fn from_process_reads_real_environment() {
        let env = WhichEnv::from_process();
        assert_eq!(env.platform, Platform::current());
        assert!(find("rscmd-definitely-missing-xyz", &env).is_none());
    }
}
