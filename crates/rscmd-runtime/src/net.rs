//! 网络基础件：URL 编码/解码、阻塞式下载器（file:// + http(s)://）。
//!
//! P3 扩展：SHA-256 校验、ETag 条件请求、Range 断点续传。
//! `download` 本身保持轻量，校验/续传以独立函数暴露，避免 Options 参数爆炸。

use sha2::{Digest, Sha256};
use std::time::Duration;

use crate::cache::DownloadCache;

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

/// URL 文本编码：按 `application/x-www-form-urlencoded` 规则转义，空格转 `%20`。
pub fn url_encode(text: &str) -> String {
    urlencoding::encode(text).into_owned()
}

/// URL 解码。
pub fn decode(s: &str) -> Result<String, std::string::FromUtf8Error> {
    urlencoding::decode(s).map(|cow| cow.into_owned())
}

/// 重试策略：默认 0 次重试，保持既有「一次失败即返回」的行为。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetryPolicy {
    /// 首次失败后最多再尝试的次数。
    pub retries: u32,
    /// 首次退避时长，之后按 2 的幂递增，单次上限 30s。
    pub backoff: Duration,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            retries: 0,
            backoff: Duration::from_millis(250),
        }
    }
}

impl RetryPolicy {
    pub fn new(retries: u32, backoff: Duration) -> Self {
        Self { retries, backoff }
    }

    /// 第 `attempt` 次失败后的等待时长（从 0 开始计数）。
    pub fn delay(&self, attempt: u32) -> Duration {
        if self.backoff.is_zero() {
            return Duration::ZERO;
        }
        let factor = 1u32.checked_shl(attempt.min(31)).unwrap_or(u32::MAX);
        self.backoff
            .saturating_mul(factor)
            .min(Duration::from_secs(30))
    }
}

/// 阻塞式下载器（不重试）。根据 URL scheme 自动选择传输层：
/// - `file://` → 本地文件读取
/// - `http://` / `https://` → reqwest blocking GET，附带超时与 30s 默认连接超时
///
/// `timeout` 同时作用于连接建立与整个请求的 read 阶段。
pub fn download(url: &str, timeout: Duration) -> Result<Vec<u8>, DownloadError> {
    download_with(url, timeout, RetryPolicy::default())
}

/// 带重试策略的下载：仅 5xx 与超时会重试（4xx、scheme、校验类错误不重试）。
pub fn download_with(
    url: &str,
    timeout: Duration,
    policy: RetryPolicy,
) -> Result<Vec<u8>, DownloadError> {
    with_retry(policy, || download_once(url, timeout))
}

fn download_once(url: &str, timeout: Duration) -> Result<Vec<u8>, DownloadError> {
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
    download_resume_with(url, offset, timeout, RetryPolicy::default())
}

/// 带重试策略的断点续传；重试判定与 [`download_with`] 一致。
pub fn download_resume_with(
    url: &str,
    offset: u64,
    timeout: Duration,
    policy: RetryPolicy,
) -> Result<Vec<u8>, DownloadError> {
    with_retry(policy, || {
        let lower = url.to_ascii_lowercase();
        if !lower.starts_with("http://") && !lower.starts_with("https://") {
            return Err(DownloadError::UnsupportedScheme(
                "断点续传仅支持 http(s)://".to_string(),
            ));
        }
        http_download(url, timeout, Some(offset))
    })
}

/// 带下载缓存与重试策略的下载：
/// - 命中缓存直接返回，不发起网络请求；
/// - 未命中时按 [`download_with`] 下载，成功后写回缓存；
/// - 缓存未启用（`None`）或写入失败时静默跳过，下载结果不受影响。
pub fn download_with_cache(
    url: &str,
    timeout: Duration,
    policy: RetryPolicy,
    cache: Option<&DownloadCache>,
) -> Result<Vec<u8>, DownloadError> {
    if let Some(data) = cache.and_then(|cache| cache.get(url)) {
        return Ok(data);
    }
    let data = download_with(url, timeout, policy)?;
    if let Some(cache) = cache {
        cache.put(url, &data);
    }
    Ok(data)
}

/// 按策略执行操作：仅可重试错误消耗重试次数，其余错误立即返回。
fn with_retry<T>(
    policy: RetryPolicy,
    mut operation: impl FnMut() -> Result<T, DownloadError>,
) -> Result<T, DownloadError> {
    let mut attempt = 0u32;
    loop {
        match operation() {
            Ok(value) => return Ok(value),
            Err(err) if attempt < policy.retries && err.is_retryable() => {
                std::thread::sleep(policy.delay(attempt));
                attempt += 1;
            }
            Err(err) => return Err(err),
        }
    }
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

    let resp = if let Some(offset) = resume_from {
        if resp.status() == reqwest::StatusCode::RANGE_NOT_SATISFIABLE {
            return Err(DownloadError::RangeNotSatisfiable(offset));
        }
        // 先按状态码分类，5xx 才能进入重试；2xx 但不是 206 才视为不支持 Range。
        let checked = resp.error_for_status().map_err(DownloadError::Reqwest)?;
        if checked.status() != reqwest::StatusCode::PARTIAL_CONTENT {
            return Err(DownloadError::RangeNotSupported);
        }
        checked
    } else {
        resp.error_for_status().map_err(DownloadError::Reqwest)?
    };
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

impl DownloadError {
    /// 统一错误码：工具侧可直接用于 `RscmdError::coded`，退出码由
    /// `ERROR_CODE_KINDS` 的 kind 决定。
    pub fn code(&self) -> &'static str {
        match self {
            DownloadError::Io(_) => "IO",
            DownloadError::Reqwest(err) if err.is_timeout() => "NET_TIMEOUT",
            DownloadError::Reqwest(err) if err.status().is_some() => "NET_HTTP_STATUS",
            DownloadError::Reqwest(_) => "NET_TRANSPORT",
            DownloadError::UnsupportedScheme(_) => "UNSUPPORTED_SCHEME",
            DownloadError::RangeNotSupported => "RANGE_NOT_SUPPORTED",
            DownloadError::RangeNotSatisfiable(_) => "RANGE_NOT_SATISFIABLE",
            DownloadError::HashMismatch { .. } => "CHECKSUM_MISMATCH",
        }
    }

    /// 是否值得重试：仅超时与 5xx；4xx、scheme、Range、校验失败都不重试。
    fn is_retryable(&self) -> bool {
        match self {
            DownloadError::Reqwest(err) if err.is_timeout() => true,
            DownloadError::Reqwest(err) => {
                err.status().is_some_and(|status| status.is_server_error())
            }
            _ => false,
        }
    }
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

    /// 按顺序回包的本地服务器，并统计实际接受的连接数；超过 500ms 无新连接则提前结束。
    fn serve_sequence(responses: Vec<&'static [u8]>) -> (String, JoinHandle<usize>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("绑定本地端口");
        let addr = listener.local_addr().expect("读取本地地址");
        listener.set_nonblocking(true).expect("设为非阻塞");
        let handle = thread::spawn(move || {
            let deadline = std::time::Instant::now() + Duration::from_millis(500);
            let mut count = 0;
            for response in responses {
                loop {
                    match listener.accept() {
                        Ok((mut stream, _)) => {
                            // Windows 上 accept 出的套接字会继承监听端的非阻塞标志，
                            // 必须显式改回阻塞，否则读/写可能立刻 WouldBlock，导致回包被截断。
                            let _ = stream.set_nonblocking(false);
                            let mut buf = [0u8; 4096];
                            let _ = stream.read(&mut buf);
                            let _ = stream.write_all(response);
                            count += 1;
                            break;
                        }
                        Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                            if std::time::Instant::now() >= deadline {
                                return count;
                            }
                            thread::sleep(Duration::from_millis(5));
                        }
                        Err(_) => return count,
                    }
                }
            }
            count
        });
        (format!("http://{addr}/data"), handle)
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
    fn url_encode_escapes_spaces_and_non_ascii() {
        assert_eq!(url_encode("a b/c"), "a%20b%2Fc");
        assert_eq!(decode(&url_encode("你好 ok")).unwrap(), "你好 ok");
    }

    #[test]
    fn download_rejects_bad_scheme() {
        let err = download("ftp://example.com", Duration::from_secs(1)).unwrap_err();
        assert!(matches!(err, DownloadError::UnsupportedScheme(_)));
        assert_eq!(err.code(), "UNSUPPORTED_SCHEME");
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
        assert_eq!(err.code(), "IO");
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
        assert_eq!(err.code(), "NET_HTTP_STATUS");
        assert!(err.to_string().contains("HTTP 错误"));
        assert!(err.source().is_some());
    }

    #[test]
    fn download_http_surfaces_truncated_body() {
        let (url, _handle) =
            serve_once(b"HTTP/1.1 200 OK\r\nContent-Length: 10\r\nConnection: close\r\n\r\nhi");
        let err = download(&url, Duration::from_secs(5)).unwrap_err();
        assert!(matches!(err, DownloadError::Reqwest(_)));
        assert_eq!(err.code(), "NET_TRANSPORT");
    }

    #[test]
    fn download_http_times_out() {
        let (url, handle) = serve_slow(Duration::from_millis(600));
        let started = std::time::Instant::now();
        let err = download(&url, Duration::from_millis(100)).unwrap_err();
        assert!(matches!(err, DownloadError::Reqwest(_)));
        assert_eq!(err.code(), "NET_TIMEOUT");
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
        // 连接失败的细分码随平台而异（reqwest 在部分平台归为超时），只约束大类。
        assert!(matches!(err.code(), "NET_TIMEOUT" | "NET_TRANSPORT"));
    }

    #[test]
    fn download_http_reports_connect_error() {
        let url = unused_local_url("http");
        let err = download(&url, Duration::from_millis(300)).unwrap_err();
        assert!(matches!(err, DownloadError::Reqwest(_)));
        assert!(matches!(err.code(), "NET_TIMEOUT" | "NET_TRANSPORT"));
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
        assert_eq!(err.code(), "CHECKSUM_MISMATCH");
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
        assert_eq!(err.code(), "UNSUPPORTED_SCHEME");
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
        assert_eq!(err.code(), "RANGE_NOT_SATISFIABLE");
        assert!(err.to_string().contains("offset=10"));
        assert!(err.source().is_none());
    }

    #[test]
    fn resume_requires_partial_content() {
        let (url, _handle) =
            serve_once(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nhi");
        let err = download_resume(&url, 1, Duration::from_secs(5)).unwrap_err();
        assert!(matches!(err, DownloadError::RangeNotSupported));
        assert_eq!(err.code(), "RANGE_NOT_SUPPORTED");
        assert!(err.to_string().contains("不支持 Range"));
    }

    #[test]
    fn retry_policy_delay_doubles_and_caps() {
        let policy = RetryPolicy::new(5, Duration::from_millis(10));
        assert_eq!(policy.delay(0), Duration::from_millis(10));
        assert_eq!(policy.delay(1), Duration::from_millis(20));
        assert_eq!(policy.delay(2), Duration::from_millis(40));
        // 指数退避封顶 30s，避免极端配置导致调用方长时间挂起。
        assert_eq!(policy.delay(20), Duration::from_secs(30));
        assert_eq!(RetryPolicy::new(5, Duration::ZERO).delay(3), Duration::ZERO);
    }

    #[test]
    fn default_policy_does_not_retry() {
        let policy = RetryPolicy::default();
        assert_eq!(policy.retries, 0);
        let (url, handle) = serve_sequence(vec![
            b"HTTP/1.1 500 Internal Server Error\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok",
        ]);
        let err = download_with(&url, Duration::from_secs(5), policy).unwrap_err();
        assert_eq!(err.code(), "NET_HTTP_STATUS");
        assert_eq!(handle.join().unwrap(), 1, "默认策略不应重试");
    }

    #[test]
    fn retries_server_errors_until_success() {
        let (url, handle) = serve_sequence(vec![
            b"HTTP/1.1 500 Internal Server Error\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok",
        ]);
        let policy = RetryPolicy::new(2, Duration::from_millis(1));
        let data = download_with(&url, Duration::from_secs(5), policy).unwrap();
        assert_eq!(data, b"ok");
        assert_eq!(handle.join().unwrap(), 3);
    }

    #[test]
    fn client_errors_are_not_retried() {
        let (url, handle) = serve_sequence(vec![
            b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok",
        ]);
        let policy = RetryPolicy::new(2, Duration::from_millis(1));
        let err = download_with(&url, Duration::from_secs(5), policy).unwrap_err();
        assert_eq!(err.code(), "NET_HTTP_STATUS");
        assert_eq!(handle.join().unwrap(), 1, "4xx 不应重试");
    }

    #[test]
    fn timeouts_are_retried() {
        let (url, handle) = serve_slow(Duration::from_millis(600));
        let started = std::time::Instant::now();
        let policy = RetryPolicy::new(1, Duration::from_millis(1));
        let err = download_with(&url, Duration::from_millis(100), policy).unwrap_err();
        assert_eq!(err.code(), "NET_TIMEOUT");
        let elapsed = started.elapsed();
        assert!(
            elapsed >= Duration::from_millis(190),
            "应发生第二次超时: {elapsed:?}"
        );
        assert!(
            elapsed < Duration::from_millis(600),
            "不应等到服务器回包: {elapsed:?}"
        );
        handle.join().unwrap();
    }

    #[test]
    fn resume_uses_retry_policy() {
        let (url, handle) = serve_sequence(vec![
            b"HTTP/1.1 500 Internal Server Error\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            b"HTTP/1.1 206 Partial Content\r\nContent-Length: 3\r\nConnection: close\r\n\r\nlo!",
        ]);
        let policy = RetryPolicy::new(1, Duration::from_millis(1));
        let data = download_resume_with(&url, 2, Duration::from_secs(5), policy).unwrap();
        assert_eq!(data, b"lo!");
        assert_eq!(handle.join().unwrap(), 2);
    }

    #[test]
    fn cached_download_does_not_refetch() {
        let dir = crate::fs::temp_dir().unwrap();
        let cache = crate::cache::DownloadCache::with_clock(
            dir.path().to_path_buf(),
            crate::cache::CachePolicy::default(),
            Box::new(std::time::SystemTime::now),
        );
        let (url, handle) =
            serve_once(b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\nConnection: close\r\n\r\nhello");
        let first = download_with_cache(
            &url,
            Duration::from_secs(5),
            RetryPolicy::default(),
            Some(&cache),
        )
        .unwrap();
        assert_eq!(first, b"hello");
        handle.join().unwrap();

        // 服务器只服务一次，第二次必须命中缓存，否则会连接失败。
        let second = download_with_cache(
            &url,
            Duration::from_secs(5),
            RetryPolicy::default(),
            Some(&cache),
        )
        .unwrap();
        assert_eq!(second, b"hello");
    }

    #[test]
    fn failed_download_is_not_cached() {
        let dir = crate::fs::temp_dir().unwrap();
        let cache = crate::cache::DownloadCache::with_clock(
            dir.path().to_path_buf(),
            crate::cache::CachePolicy::default(),
            Box::new(std::time::SystemTime::now),
        );
        let (url, _handle) =
            serve_once(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
        let err = download_with_cache(
            &url,
            Duration::from_secs(5),
            RetryPolicy::default(),
            Some(&cache),
        )
        .unwrap_err();
        assert_eq!(err.code(), "NET_HTTP_STATUS");
        assert!(cache.get(&url).is_none(), "失败结果不应写入缓存");
    }

    #[test]
    fn unusable_cache_degrades_to_plain_download() {
        let dir = crate::fs::temp_dir().unwrap();
        // 用普通文件占据缓存根目录，构造出的缓存句柄读写都会静默失败。
        let blocked = dir.path().join("blocked");
        std::fs::write(&blocked, b"file").unwrap();
        let cache = crate::cache::DownloadCache::with_clock(
            blocked,
            crate::cache::CachePolicy::default(),
            Box::new(std::time::SystemTime::now),
        );
        let (url, handle) =
            serve_once(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok");
        let data = download_with_cache(
            &url,
            Duration::from_secs(5),
            RetryPolicy::default(),
            Some(&cache),
        )
        .unwrap();
        assert_eq!(data, b"ok");
        handle.join().unwrap();
        assert!(cache.get(&url).is_none());
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
