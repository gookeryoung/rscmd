//! 文件系统基础件：原子写、临时目录、路径规范化、文本探测。

use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

static TMP_COUNTER: AtomicUsize = AtomicUsize::new(0);

/// 文本文件常见扩展名白名单（小写）。无扩展名或不在白名单内的按二进制处理。
const TEXT_EXTENSIONS: &[&str] = &[
    "txt",
    "md",
    "rst",
    "log",
    "csv",
    "tsv",
    "json",
    "yaml",
    "yml",
    "toml",
    "xml",
    "html",
    "htm",
    "css",
    "scss",
    "less",
    "js",
    "ts",
    "jsx",
    "tsx",
    "py",
    "rb",
    "go",
    "rs",
    "java",
    "kt",
    "c",
    "h",
    "cpp",
    "hpp",
    "cs",
    "php",
    "sh",
    "bash",
    "zsh",
    "fish",
    "bat",
    "ps1",
    "conf",
    "cfg",
    "ini",
    "properties",
    "env",
    "sql",
    "proto",
    "graphql",
    "gql",
    "diff",
    "patch",
    "svg",
    "lock",
    "toml",
    "yml",
];

/// 一次性写文件到 `path`。采用“先写临时文件再 rename”的原子策略：即使进程在写入中途崩溃，
/// 目标文件仍保持上一次的完整内容。父目录不存在时自动创建。
///
/// 临时文件放在 `path` 的父目录下，以 `.rscmd-tmp-<随机>` 为前缀；写入成功后 rename 覆盖。
pub fn atomic_write(path: &Path, contents: &[u8]) -> io::Result<()> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent)?;
    }
    let tmp_path = path.with_extension(format!(
        "{}.rscmd-tmp-{}",
        path.extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("dat"),
        rand_suffix()
    ));
    let result = write_all(&tmp_path, contents).and_then(|_| fs::rename(&tmp_path, path));
    if result.is_err() {
        let _ = fs::remove_file(&tmp_path);
    }
    result
}

fn write_all(path: &Path, contents: &[u8]) -> io::Result<()> {
    let mut file = fs::File::create(path)?;
    file.write_all(contents)?;
    file.flush()?;
    Ok(())
}

fn rand_suffix() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() % 1_000_000)
        .unwrap_or(0);
    format!("{nanos:x}")
}

/// 在指定根目录下创建临时目录。返回的 `TempDir` 会在 Drop 时递归清理。
pub fn temp_dir() -> io::Result<TempDir> {
    let base = std::env::temp_dir();
    temp_dir_in(&base)
}

/// 在指定父目录下创建临时目录。
pub fn temp_dir_in(parent: &Path) -> io::Result<TempDir> {
    fs::create_dir_all(parent)?;
    let seq = TMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let name = format!("rscmd-{}-{}-{seq}", std::process::id(), rand_suffix());
    let path = parent.join(name);
    fs::create_dir(&path)?;
    Ok(TempDir { path })
}

/// 拥有式临时目录。Drop 时自动递归删除；也可以显式调用 [`TempDir::close`] 提前清理。
///
/// 若删除失败（目录已被外部删除、权限不足等），Drop 时静默忽略——避免在析构链里 panic。
pub struct TempDir {
    path: PathBuf,
}

impl TempDir {
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn close(self) -> io::Result<()> {
        let path = self.path.clone();
        std::mem::forget(self);
        fs::remove_dir_all(&path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

/// 路径规范化：
/// - 把分隔符统一为 `/`（输出始终用正斜杠）；
/// - 消除连续分隔符；
/// - 解析 `.` / `..` 段（不做符号链接解析，保持纯字符串语义）；
/// - 保留 Windows 盘符前缀（`C:/foo`）、Unix 根 `/` 与相对路径原样。
///
/// 跨平台语义：输入既可以是 `a\b\..\c` 也可以是 `a/b/../c`，输出统一为正斜杠。
pub fn normalize_path(path: &Path) -> PathBuf {
    let as_str = path.to_string_lossy();
    let mut parts: Vec<String> = as_str
        .replace('\\', "/")
        .split('/')
        .filter(|seg| !seg.is_empty())
        .map(String::from)
        .collect();

    let mut stack: Vec<String> = Vec::new();
    let mut leading_dots: Vec<String> = Vec::new();
    for seg in parts.drain(..) {
        match seg.as_str() {
            "." => {}
            ".." => {
                if let Some(top) = stack.last() {
                    if top == ".." {
                        stack.push("..".to_string());
                    } else {
                        stack.pop();
                    }
                } else {
                    leading_dots.push("..".to_string());
                }
            }
            _ => {
                stack.push(seg);
            }
        }
    }
    leading_dots.extend_from_slice(&stack);
    PathBuf::from(leading_dots.join("/"))
}

/// 轻量文本文件判断：
/// - 扩展名在白名单内 → 视为文本；
/// - 无扩展名 → 读前 2KB，含 null 字节视为二进制；
/// - 其他扩展名 → 读前 2KB，UTF-8 解码无错误视为文本。
pub fn is_text_file(path: &Path) -> io::Result<bool> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .unwrap_or_default();

    let mut buf = Vec::with_capacity(2048);
    let mut file = fs::File::open(path)?;
    let n = std::io::Read::by_ref(&mut file)
        .take(2048)
        .read_to_end(&mut buf)?;
    if n == 0 {
        return Ok(true);
    }
    if buf.contains(&0) {
        return Ok(false);
    }
    if TEXT_EXTENSIONS.contains(&ext.as_str()) {
        return Ok(true);
    }
    Ok(std::str::from_utf8(&buf).is_ok())
}

/// 读文件，返回 UTF-8 文本。失败时给出文件名提示。
pub fn read_text(path: &Path) -> io::Result<String> {
    let bytes = fs::read(path)?;
    String::from_utf8(bytes).map_err(|err| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("{} 不是有效的 UTF-8: {err}", path.display()),
        )
    })
}

/// 递归遍历目录。回调收到每个文件的路径；返回 false 时停止当前分支的递归。
/// 忽略 `.rscmd-tmp-*` 临时文件。
pub fn walk_files<F>(root: &Path, mut visitor: F) -> io::Result<()>
where
    F: FnMut(&Path) -> bool,
{
    walk_inner(root, &mut visitor)
}

fn walk_inner<F>(dir: &Path, visitor: &mut F) -> io::Result<()>
where
    F: FnMut(&Path) -> bool,
{
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if name_str.contains(".rscmd-tmp-") {
            continue;
        }
        if path.is_dir() {
            walk_inner(&path, visitor)?;
        } else if path.is_file() && !visitor(&path) {
            break;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atomic_write_roundtrip() {
        let dir = temp_dir().unwrap();
        let path = dir.path().join("out.txt");
        atomic_write(&path, b"hello world").unwrap();
        let content = fs::read_to_string(&path).unwrap();
        assert_eq!(content, "hello world");
    }

    #[test]
    fn atomic_write_creates_parent_dir() {
        let dir = temp_dir().unwrap();
        let path = dir.path().join("a/b/c/d.txt");
        atomic_write(&path, b"nested").unwrap();
        assert!(path.exists());
    }

    #[test]
    fn temp_dir_cleans_up_on_drop() {
        let path;
        {
            let dir = temp_dir().unwrap();
            path = dir.path().to_path_buf();
            fs::write(dir.path().join("x.txt"), b"hi").unwrap();
            assert!(path.exists());
        }
        assert!(!path.exists());
    }

    #[test]
    fn temp_dir_close_explicit() {
        let dir = temp_dir().unwrap();
        let path = dir.path().to_path_buf();
        dir.close().unwrap();
        assert!(!path.exists());
    }

    #[test]
    fn normalize_path_handles_dots() {
        assert_eq!(
            normalize_path(Path::new("a/b/./c/../d")),
            Path::new("a/b/d")
        );
        assert_eq!(normalize_path(Path::new("../a")), Path::new("../a"));
        assert_eq!(normalize_path(Path::new("a/../..")), Path::new(".."));
    }

    #[test]
    fn read_text_succeeds_on_utf8() {
        let dir = temp_dir().unwrap();
        let path = dir.path().join("hello.txt");
        fs::write(&path, "你好").unwrap();
        assert_eq!(read_text(&path).unwrap(), "你好");
    }

    #[test]
    fn read_text_rejects_non_utf8() {
        let dir = temp_dir().unwrap();
        let path = dir.path().join("bad.txt");
        fs::write(&path, b"hi \xff\xfe").unwrap();
        let err = read_text(&path).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn is_text_follows_extension() {
        let dir = temp_dir().unwrap();
        let json = dir.path().join("data.json");
        fs::write(&json, "{\"a\":1}").unwrap();
        assert!(is_text_file(&json).unwrap());
    }

    #[test]
    fn is_text_detects_binary() {
        let dir = temp_dir().unwrap();
        let bin = dir.path().join("data.unknown");
        fs::write(&bin, [0u8; 100]).unwrap();
        assert!(!is_text_file(&bin).unwrap());
    }

    #[test]
    fn walk_files_visits_each() {
        let dir = temp_dir().unwrap();
        fs::create_dir(dir.path().join("sub")).unwrap();
        fs::write(dir.path().join("a.txt"), b"").unwrap();
        fs::write(dir.path().join("sub/b.txt"), b"").unwrap();
        let mut paths = Vec::new();
        walk_files(dir.path(), |path| {
            paths.push(path.to_path_buf());
            true
        })
        .unwrap();
        assert_eq!(paths.len(), 2);
    }

    #[test]
    fn walk_files_skips_tmp() {
        let dir = temp_dir().unwrap();
        fs::write(dir.path().join("real.txt"), b"").unwrap();
        fs::write(dir.path().join("fake.rscmd-tmp-abc123"), b"").unwrap();
        let mut paths = Vec::new();
        walk_files(dir.path(), |path| {
            paths.push(path.file_name().unwrap().to_string_lossy().into_owned());
            true
        })
        .unwrap();
        assert_eq!(paths, vec!["real.txt"]);
    }
}
