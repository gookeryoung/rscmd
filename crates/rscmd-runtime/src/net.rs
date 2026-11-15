//! 网络基础件：URL 编码/解码、阻塞式下载器（file:// + http(s)://）。

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
        let path = path.trim_start_matches('/');
        return std::fs::read(path).map_err(DownloadError::Io);
    }
    if lower.starts_with("http://") || lower.starts_with("https://") {
        return http_download(url, timeout);
    }
    Err(DownloadError::UnsupportedScheme(
        "仅支持 file://、http://、https://".to_string(),
    ))
}

fn http_download(url: &str, timeout: Duration) -> Result<Vec<u8>, DownloadError> {
    let client = reqwest::blocking::Client::builder()
        .connect_timeout(timeout.min(Duration::from_secs(30)))
        .timeout(timeout)
        .user_agent(concat!("rscmd/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(DownloadError::Reqwest)?;

    let resp = client
        .get(url)
        .send()
        .and_then(|r| r.error_for_status())
        .map_err(DownloadError::Reqwest)?;

    let bytes = resp.bytes().map_err(DownloadError::Reqwest)?;
    Ok(bytes.to_vec())
}

#[derive(Debug)]
pub enum DownloadError {
    Io(std::io::Error),
    Reqwest(reqwest::Error),
    UnsupportedScheme(String),
}

impl std::fmt::Display for DownloadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DownloadError::Io(err) => write!(f, "I/O 错误: {err}"),
            DownloadError::Reqwest(err) => write!(f, "HTTP 错误: {err}"),
            DownloadError::UnsupportedScheme(msg) => write!(f, "不支持的 URL scheme: {msg}"),
        }
    }
}

impl std::error::Error for DownloadError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            DownloadError::Io(err) => Some(err),
            DownloadError::Reqwest(err) => Some(err),
            DownloadError::UnsupportedScheme(_) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
