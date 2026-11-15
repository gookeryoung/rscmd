//! 网络基础件：URL 编码/解码、简易阻塞式下载器（P1 占位，P2 补完整 HTTP 客户端）。

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

/// 简易阻塞式下载器（P1 占位）。
///
/// P1 不引入 reqwest/ureq 等 HTTP 客户端；该函数当前只对 `file://` 形式的 URL 有效，
/// 对 `http://` / `https://` 返回 [`NotImplemented`] 错误。
/// 真正的 HTTP 客户端（超时、重试、下载校验、流式）在 P2 引入。
pub fn download(url: &str, timeout: Duration) -> Result<Vec<u8>, DownloadError> {
    if !url.starts_with("file://") {
        return Err(DownloadError::NotImplemented(
            "HTTP/HTTPS 下载将在 P2 用 reqwest 实现；当前仅支持 file://".to_string(),
        ));
    }
    let path = url
        .strip_prefix("file://")
        .unwrap_or(url)
        .trim_start_matches('/');
    let _ = timeout; // file:// 读文件不超时（std::fs 本身不支持 timeout）
    std::fs::read(path).map_err(DownloadError::Io)
}

#[derive(Debug)]
pub enum DownloadError {
    Io(std::io::Error),
    NotImplemented(String),
}

impl std::fmt::Display for DownloadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DownloadError::Io(err) => write!(f, "I/O 错误: {err}"),
            DownloadError::NotImplemented(msg) => write!(f, "未实现: {msg}"),
        }
    }
}

impl std::error::Error for DownloadError {}

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
    fn download_rejects_http() {
        let err = download("https://example.com", Duration::from_secs(1)).unwrap_err();
        assert!(matches!(err, DownloadError::NotImplemented(_)));
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
}
