//! `rscmd-gui` 的纯逻辑：预览格式化、缩放计算与中文字体探测。
//!
//! 本模块不依赖窗口库，默认构建（未启用 `gui` feature）也会编译并单测，
//! 以便把可验证的逻辑与 eframe 事件循环隔离开。

use std::path::{Path, PathBuf};

/// 预览倍率下限。
pub const MIN_ZOOM: f32 = 0.1;
/// 预览倍率上限。
pub const MAX_ZOOM: f32 = 16.0;
/// 每次放大/缩小的步长。
pub const ZOOM_STEP: f32 = 1.25;
/// 「适应窗口」允许的最大放大倍率。
pub const FIT_MAX_UPSCALE: f32 = 8.0;
/// 十六进制预览的字节上限。
pub const HEX_PREVIEW_LIMIT: usize = 4096;
/// 文本预览的字节上限。
pub const TEXT_PREVIEW_LIMIT: usize = 512 * 1024;

/// 把倍率限制在 [`MIN_ZOOM`] ~ [`MAX_ZOOM`] 之间。
pub fn clamp_zoom(value: f32) -> f32 {
    if value.is_nan() {
        return 1.0;
    }
    value.clamp(MIN_ZOOM, MAX_ZOOM)
}

/// 放大一档。
pub fn zoom_in(value: f32) -> f32 {
    clamp_zoom(value * ZOOM_STEP)
}

/// 缩小一档。
pub fn zoom_out(value: f32) -> f32 {
    clamp_zoom(value / ZOOM_STEP)
}

/// 计算「适应窗口」倍率：完整显示图像，且放大不超过 [`FIT_MAX_UPSCALE`]。
///
/// 任一尺寸无效时返回 `1.0`。
pub fn fit_zoom(image_width: u32, image_height: u32, view_width: f32, view_height: f32) -> f32 {
    if image_width == 0 || image_height == 0 || view_width <= 0.0 || view_height <= 0.0 {
        return 1.0;
    }
    let scale = (view_width / image_width as f32).min(view_height / image_height as f32);
    clamp_zoom(scale.min(FIT_MAX_UPSCALE))
}

/// 是否为适合文本预览的资源类型（与 fcmd 的白名单一致）。
pub fn is_text_type(type_id: u32) -> bool {
    matches!(type_id, 4 | 5 | 6 | 9 | 10 | 11 | 16 | 17 | 23 | 24)
}

/// 解码文本资源：UTF-16 BOM / 零字节比例 / UTF-8 / 替换字符。
///
/// POC 不引入 GBK 解码；无法按 UTF-8 解码时使用替换字符，后续可在正式阶段
/// 按决策点补 `encoding_rs`。
pub fn decode_text(data: &[u8]) -> String {
    if data.is_empty() {
        return "(空资源)".to_string();
    }
    if data.starts_with(&[0xFF, 0xFE]) {
        return utf16_lossy(&data[2..], true);
    }
    if data.starts_with(&[0xFE, 0xFF]) {
        return utf16_lossy(&data[2..], false);
    }
    let zeros = data.iter().filter(|byte| **byte == 0).count();
    if zeros * 10 > data.len() * 3 {
        return utf16_lossy(data, true);
    }
    match std::str::from_utf8(data) {
        Ok(text) => text.to_string(),
        Err(_) => {
            // 无 BOM 的 UTF-16LE 中文字符高字节通常非零，零字节比例判定会漏掉；
            // UTF-8 失败后再尝试一次 UTF-16LE，能完整解码时优先采用。
            if data.len() % 2 == 0 {
                let candidate = utf16_lossy(data, true);
                if !candidate.contains('\u{FFFD}') {
                    return candidate;
                }
            }
            String::from_utf8_lossy(data).into_owned()
        }
    }
}

fn utf16_lossy(bytes: &[u8], little_endian: bool) -> String {
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|pair| {
            if little_endian {
                u16::from_le_bytes([pair[0], pair[1]])
            } else {
                u16::from_be_bytes([pair[0], pair[1]])
            }
        })
        .collect();
    String::from_utf16_lossy(&units)
}

/// 格式化十六进制预览（每行 16 字节，含 ASCII 列，超长截断）。
pub fn format_hex(data: &[u8], limit: usize) -> String {
    if data.is_empty() {
        return "(空资源)".to_string();
    }
    let limit = limit.max(1);
    let view = &data[..data.len().min(limit)];
    let mut lines = Vec::new();
    for (index, chunk) in view.chunks(16).enumerate() {
        let hex_part = chunk
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<Vec<_>>()
            .join(" ");
        let ascii_part: String = chunk
            .iter()
            .map(|byte| {
                if (32..127).contains(byte) {
                    *byte as char
                } else {
                    '.'
                }
            })
            .collect();
        lines.push(format!("{:08x}  {hex_part:<47}  {ascii_part}", index * 16));
    }
    if data.len() > limit {
        lines.push(format!("... 仅显示前 {limit} 字节，共 {} 字节", data.len()));
    }
    lines.join("\n")
}

/// 中文字体候选名（按优先级排列）。
pub fn cjk_font_candidates(fonts_dir: &Path) -> Vec<PathBuf> {
    [
        "simhei.ttf",
        "Deng.ttf",
        "msyh.ttc",
        "simsun.ttc",
        "simkai.ttf",
        "simfang.ttf",
    ]
    .iter()
    .map(|name| fonts_dir.join(name))
    .collect()
}

/// 在字体目录中查找第一个可用的中文字体。
pub fn find_cjk_font(fonts_dir: &Path) -> Option<PathBuf> {
    cjk_font_candidates(fonts_dir)
        .into_iter()
        .find(|path| path.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zoom_is_clamped() {
        assert_eq!(zoom_in(MAX_ZOOM), MAX_ZOOM);
        assert_eq!(zoom_out(MIN_ZOOM), MIN_ZOOM);
        assert_eq!(clamp_zoom(f32::NAN), 1.0);
    }

    #[test]
    fn fit_zoom_keeps_aspect_and_caps_upscale() {
        assert_eq!(fit_zoom(100, 100, 400.0, 400.0), 4.0);
        assert_eq!(fit_zoom(100, 100, 4000.0, 4000.0), FIT_MAX_UPSCALE);
        assert_eq!(fit_zoom(100, 100, 0.0, 400.0), 1.0);
        assert_eq!(fit_zoom(0, 100, 400.0, 400.0), 1.0);
    }

    #[test]
    fn hex_preview_has_offset_and_ascii() {
        let text = format_hex(b"A\x00", 16);
        assert!(text.starts_with("00000000  41 00"));
        assert!(text.ends_with("A."));
    }

    #[test]
    fn hex_preview_truncates() {
        let text = format_hex(&[0u8; 32], 16);
        assert!(text.contains("共 32 字节"));
    }

    #[test]
    fn decodes_utf8_and_utf16() {
        assert_eq!(decode_text("中文".as_bytes()), "中文");
        assert_eq!(decode_text(&[0xFF, 0xFE, 0x2D, 0x4E, 0x87, 0x65]), "中文");
        assert_eq!(decode_text(&[0x2D, 0x4E, 0x87, 0x65]), "中文");
        assert_eq!(decode_text(b""), "(空资源)");
    }

    #[test]
    fn font_candidates_are_ordered() {
        let candidates = cjk_font_candidates(Path::new("C:/Windows/Fonts"));
        assert_eq!(candidates[0], Path::new("C:/Windows/Fonts/simhei.ttf"));
        assert_eq!(candidates.len(), 6);
    }

    #[test]
    fn text_types_match_fcmd_whitelist() {
        for type_id in [4, 5, 6, 9, 10, 11, 16, 17, 23, 24] {
            assert!(is_text_type(type_id));
        }
        assert!(!is_text_type(2));
        assert!(!is_text_type(3));
    }
}
