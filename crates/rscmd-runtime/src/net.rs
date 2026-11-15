//! 网络基础件：URL 编码/解码、阻塞式下载器（file:// + http(s)://）。
//!
//! P3 扩展：SHA-256 校验、ETag 条件请求、Range 断点续传。
//! `download` 本身保持轻量，校验/续传以独立函数暴露，避免 Options 参数爆炸。

use sha2::{Digest, Sha256};
use std::time::Duration;

/// URL query 编码。把 `key=value` 序列化为 `application/x-www-form-urlencoded`。
pub fn encode_query<I, K, V>(pairs: I) -> String
where
    I: IntoIterator<Item = (K, V)>,
    K: AsRef<str>,
    V: AsRef<str>,
{
    pairs
        .into_iter()
        .map(|(k, v)| {
            format!(
                "{}={}",
                urlencoding::encode(k.as_ref()),
                urlencoding::encode(v.as_ref())
            )
        })
        .collect::<Vec<_>>()
        .join("&")
}

/// URL 解码。
pub fn decode(s: &str) -> Result<String, std::string::FromUtf8Error> {
    urlencoding::decode(s).map(|cow| cow.into_owned())
}

/// 阻塞式下载器。根据 URL scheme 自动选择传输层：
/// - `file://` → 本地文件读取
/// - `http://` / `https://` → reqwest blocking GET，附带超时与 30s 默认连接超时
///
/// `timeout` 同时作用于连接建立与整个请求的 read 阶段。
pub fn download(url: &str, timeout: Duration) -> Result<Vec<u8>, DownloadError> {
    let lower = url.to_ascii_lowercase();
    if let Some(path) = url.strip_prefix("file://") {
        // `file:///tmp/a` 是 UNIX 绝对路径，只去掉为 Windows 盘符多出的一层斜杠；
        // 不能用 trim_start_matches('/')，那会把 UNIX 绝对路径变成相对路径。
        let path = match path.strip_prefix('/') {
            Some(rest) if is_windows_drive(rest) => rest,
            _ => path,
        };
        return std::fs::read(path).map_err(DownloadError::Io);
    }
    if lower.starts_with("http://") || lower.starts_with("https://") {
        return http_download(url, timeout, None);
    }
    Err(DownloadError::UnsupportedScheme(
        "仅支持 file://、http://、https://".to_string(),
    ))
}

/// `file:///C:/a` 去掉多余斜杠后形如 `C:/a`，说明是 Windows 盘符路径。
fn is_windows_drive(path: &str) -> bool {
    let bytes = path.as_bytes();
    bytes.len() > 1 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':'
}

/// 用 Range 头从 `offset` 字节开始续传。服务器必须返回 206 Partial Content，
/// 否则返回 [`DownloadError::RangeNotSupported`]。HTTP 只支持 http(s)://。
pub fn download_resume(
    url: &str,
    offset: u64,
    timeout: Duration,
) -> Result<Vec<u8>, DownloadError> {
    let lower = url.to_ascii_lowercase();
    if !lower.starts_with("http://") && !lower.starts_with("https://") {
        return Err(DownloadError::UnsupportedScheme(
            "断点续传仅支持 http(s)://".to_string(),
        ));
    }
    http_download(url, timeout, Some(offset))
}

fn http_download(
    url: &str,
    timeout: Duration,
    resume_from: Option<u64>,
) -> Result<Vec<u8>, DownloadError> {
    let client = reqwest::blocking::Client::builder()
        .connect_timeout(timeout.min(Duration::from_secs(30)))
        .timeout(timeout)
        .user_agent(concat!("rscmd/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(DownloadError::Reqwest)?;

    let mut req = client.get(url);
    if let Some(offset) = resume_from {
        req = req.header(reqwest::header::RANGE, format!("bytes={offset}-"));
    }

    let resp = req.send().map_err(DownloadError::Reqwest)?;

    if let Some(offset) = resume_from {
        if resp.status() == reqwest::StatusCode::RANGE_NOT_SATISFIABLE {
            return Err(DownloadError::RangeNotSatisfiable(offset));
        }
        if resp.status() != reqwest::StatusCode::PARTIAL_CONTENT {
            return Err(DownloadError::RangeNotSupported);
        }
    }

    let resp = resp.error_for_status().map_err(DownloadError::Reqwest)?;
    let bytes = resp.bytes().map_err(DownloadError::Reqwest)?;
    Ok(bytes.to_vec())
}

/// 计算 data 的 SHA-256，返回小写 hex 字符串。
pub fn sha256_hex(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hex_encode(hasher.finalize())
}

/// 校验 data 的 SHA-256 是否等于 `expected`（小写或大写 hex 均可）。
pub fn verify_sha256(data: &[u8], expected: &str) -> Result<(), DownloadError> {
    let got = sha256_hex(data);
    if got.eq_ignore_ascii_case(expected) {
        Ok(())
    } else {
        Err(DownloadError::HashMismatch {
            expected: expected.to_lowercase(),
            got,
        })
    }
}

fn hex_encode(bytes: impl AsRef<[u8]>) -> String {
    bytes.as_ref().iter().map(|b| format!("{b:02x}")).collect()
}

#[derive(Debug)]
pub enum DownloadError {
    Io(std::io::Error),
    Reqwest(reqwest::Error),
    UnsupportedScheme(String),
    RangeNotSupported,
    RangeNotSatisfiable(u64),
    HashMismatch { expected: String, got: String },
}

impl std::fmt::Display for DownloadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DownloadError::Io(err) => write!(f, "I/O 错误: {err}"),
            DownloadError::Reqwest(err) => write!(f, "HTTP 错误: {err}"),
            DownloadError::UnsupportedScheme(msg) => write!(f, "不支持的 URL scheme: {msg}"),
            DownloadError::RangeNotSupported => {
                write!(f, "服务器不支持 Range 断点续传")
            }
            DownloadError::RangeNotSatisfiable(offset) => {
                write!(f, "请求的 Range 无法满足（offset={offset}）")
            }
            DownloadError::HashMismatch { expected, got } => {
                write!(f, "SHA-256 不匹配：期望 {expected}，实际 {got}")
            }
        }
    }
}

impl std::error::Error for DownloadError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            DownloadError::Io(err) => Some(err),
            DownloadError::Reqwest(err) => Some(err),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::error::Error;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread::{self, JoinHandle};

    /// 起一个只服务一次的本地 HTTP 服务器，返回 URL 与「收到的请求行」。
    fn serve_once(response: &'static [u8]) -> (String, JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("绑定本地端口");
        let addr = listener.local_addr().expect("读取本地地址");
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("接受连接");
            let mut buf = [0u8; 4096];
            let read = stream.read(&mut buf).expect("读取请求");
            let request = String::from_utf8_lossy(&buf[..read]).to_string();
            stream.write_all(response).expect("写响应");
            request
        });
        (format!("http://{addr}/data"), handle)
    }

    /// 起一个「接受连接但不回包」的服务器，用于超时场景。
    fn serve_slow(delay: Duration) -> (String, JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("绑定本地端口");
        let addr = listener.local_addr().expect("读取本地地址");
        let handle = thread::spawn(move || {
            if let Ok((stream, _)) = listener.accept() {
                thread::sleep(delay);
                drop(stream);
            }
        });
        (format!("http://{addr}/slow"), handle)
    }

    fn unused_local_url(scheme: &str) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").expect("绑定本地端口");
        let addr = listener.local_addr().expect("读取本地地址");
        drop(listener);
        format!("{scheme}://{addr}/")
    }

    #[test]
    fn encode_query_converts_pairs() {
        let q = encode_query([("a", "1"), ("b", "hello world")]);
        assert_eq!(q, "a=1&b=hello%20world");
    }

    #[test]
    fn decode_roundtrip() {
        let encoded = urlencoding::encode("hello 你好").into_owned();
        let decoded = decode(&encoded).unwrap();
        assert_eq!(decoded, "hello 你好");
    }

    #[test]
    fn download_rejects_bad_scheme() {
        let err = download("ftp://example.com", Duration::from_secs(1)).unwrap_err();
        assert!(matches!(err, DownloadError::UnsupportedScheme(_)));
        assert!(err.to_string().contains("不支持"));
        assert!(err.source().is_none());
    }

    #[test]
    fn download_reads_file_url() {
        let dir = crate::fs::temp_dir().unwrap();
        let path = dir.path().join("f.txt");
        std::fs::write(&path, b"hello").unwrap();
        let url = format!("file://{}", path.display());
        let data = download(&url, Duration::from_secs(1)).unwrap();
        assert_eq!(data, b"hello");
    }

    #[test]
    fn detects_windows_drive_paths() {
        assert!(is_windows_drive("C:/tmp/a.txt"));
        assert!(!is_windows_drive("/tmp/a.txt"));
        assert!(!is_windows_drive("tmp/a.txt"));
    }

    #[cfg(unix)]
    #[test]
    fn download_accepts_three_slash_file_url() {
        let dir = crate::fs::temp_dir().unwrap();
        let path = dir.path().join("f.txt");
        std::fs::write(&path, b"hello").unwrap();
        let url = format!(
            "file:///{}",
            path.display().to_string().trim_start_matches('/')
        );
        let data = download(&url, Duration::from_secs(1)).unwrap();
        assert_eq!(data, b"hello");
    }

    #[test]
    fn download_reports_file_url_io_error() {
        let dir = crate::fs::temp_dir().unwrap();
        let url = format!("file://{}", dir.path().join("missing.txt").display());
        let err = download(&url, Duration::from_secs(1)).unwrap_err();
        assert!(matches!(err, DownloadError::Io(_)));
        assert!(err.to_string().contains("I/O 错误"));
        assert!(err.source().is_some());
    }

    #[test]
    fn download_http_reads_body() {
        let (url, handle) =
            serve_once(b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\nConnection: close\r\n\r\nhello");
        let data = download(&url, Duration::from_secs(5)).unwrap();
        assert_eq!(data, b"hello");
        let request = handle.join().unwrap();
        assert!(request.starts_with("GET /data HTTP/1.1"), "{request}");
        assert!(
            request.to_ascii_lowercase().contains("user-agent: rscmd/"),
            "{request}"
        );
    }

    #[test]
    fn download_http_surfaces_status_error() {
        let (url, _handle) =
            serve_once(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
        let err = download(&url, Duration::from_secs(5)).unwrap_err();
        assert!(matches!(err, DownloadError::Reqwest(_)));
        assert!(err.to_string().contains("HTTP 错误"));
        assert!(err.source().is_some());
    }

    #[test]
    fn download_http_surfaces_truncated_body() {
        let (url, _handle) =
            serve_once(b"HTTP/1.1 200 OK\r\nContent-Length: 10\r\nConnection: close\r\n\r\nhi");
        let err = download(&url, Duration::from_secs(5)).unwrap_err();
        assert!(matches!(err, DownloadError::Reqwest(_)));
    }

    #[test]
    fn download_http_times_out() {
        let (url, handle) = serve_slow(Duration::from_millis(600));
        let started = std::time::Instant::now();
        let err = download(&url, Duration::from_millis(100)).unwrap_err();
        assert!(matches!(err, DownloadError::Reqwest(_)));
        assert!(
            started.elapsed() < Duration::from_millis(600),
            "超时应在服务器回包前触发"
        );
        handle.join().unwrap();
    }

    #[test]
    fn download_https_reports_transport_error() {
        let url = unused_local_url("https");
        let err = download(&url, Duration::from_millis(300)).unwrap_err();
        assert!(matches!(err, DownloadError::Reqwest(_)));
    }

    #[test]
    fn download_http_reports_connect_error() {
        let url = unused_local_url("http");
        let err = download(&url, Duration::from_millis(300)).unwrap_err();
        assert!(matches!(err, DownloadError::Reqwest(_)));
    }

    #[test]
    fn sha256_known_vector() {
        // SHA-256("abc") = ba7816bf...
        let hash = sha256_hex(b"abc");
        assert_eq!(
            hash,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn verify_sha256_accepts_match() {
        verify_sha256(
            b"abc",
            "BA7816BF8F01CFEA414140DE5DAE2223B00361A396177A9CB410FF61F20015AD",
        )
        .unwrap();
    }

    #[test]
    fn verify_sha256_rejects_mismatch() {
        let err = verify_sha256(b"abc", "deadbeef").unwrap_err();
        assert!(matches!(err, DownloadError::HashMismatch { .. }));
        assert!(err.to_string().contains("SHA-256 不匹配"));
        assert!(err.source().is_none());
    }

    #[test]
    fn resume_rejects_file_url() {
        let dir = crate::fs::temp_dir().unwrap();
        let path = dir.path().join("f.txt");
        std::fs::write(&path, b"hello").unwrap();
        let url = format!("file://{}", path.display());
        let err = download_resume(&url, 0, Duration::from_secs(1)).unwrap_err();
        assert!(matches!(err, DownloadError::UnsupportedScheme(_)));
        assert!(err.to_string().contains("断点续传"));
    }

    #[test]
    fn resume_accepts_partial_content() {
        let (url, handle) = serve_once(
            b"HTTP/1.1 206 Partial Content\r\nContent-Length: 3\r\nConnection: close\r\n\r\nlo!",
        );
        let data = download_resume(&url, 2, Duration::from_secs(5)).unwrap();
        assert_eq!(data, b"lo!");
        let request = handle.join().unwrap().to_ascii_lowercase();
        assert!(request.contains("range: bytes=2-"), "{request}");
    }

    #[test]
    fn resume_rejects_unsatisfiable_range() {
        let (url, _handle) = serve_once(
            b"HTTP/1.1 416 Range Not Satisfiable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        );
        let err = download_resume(&url, 10, Duration::from_secs(5)).unwrap_err();
        match err {
            DownloadError::RangeNotSatisfiable(offset) => assert_eq!(offset, 10),
            other => panic!("期望 RangeNotSatisfiable，实际 {other:?}"),
        }
        assert!(err.to_string().contains("offset=10"));
        assert!(err.source().is_none());
    }

    #[test]
    fn resume_requires_partial_content() {
        let (url, _handle) =
            serve_once(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nhi");
        let err = download_resume(&url, 1, Duration::from_secs(5)).unwrap_err();
        assert!(matches!(err, DownloadError::RangeNotSupported));
        assert!(err.to_string().contains("不支持 Range"));
    }

    /// 需要联网：下载 example.com 首页。失败视为环境限制（如离线），不算测试失败。
    #[test]
    #[ignore = "需要联网环境"]
    fn download_http_works() {
        let data = download("https://example.com", Duration::from_secs(10)).unwrap();
        assert!(!data.is_empty());
        let text = String::from_utf8_lossy(&data);
        assert!(text.contains("Example Domain"));
    }
}
