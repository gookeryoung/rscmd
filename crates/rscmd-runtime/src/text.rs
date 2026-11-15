//! 文本基础操作：分词、命名风格转换、统计。

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaseStyle {
    Snake,
    Camel,
    Pascal,
    Kebab,
}

/// 把任意标识符拆成小写单词。支持分隔符、驼峰与连续大写（如 HTTPServer）。
pub fn split_words(input: &str) -> Vec<String> {
    let chars: Vec<char> = input.chars().collect();
    let mut words = Vec::new();
    let mut current = String::new();

    for (index, ch) in chars.iter().copied().enumerate() {
        if !ch.is_alphanumeric() {
            flush(&mut current, &mut words);
            continue;
        }

        if ch.is_uppercase() && index > 0 {
            let prev = chars[index - 1];
            let next_is_lower = chars
                .get(index + 1)
                .map(|next| next.is_lowercase())
                .unwrap_or(false);
            if prev.is_lowercase() || prev.is_numeric() || (prev.is_uppercase() && next_is_lower) {
                flush(&mut current, &mut words);
            }
        }

        current.push(ch);
    }
    flush(&mut current, &mut words);

    words
}

fn flush(current: &mut String, words: &mut Vec<String>) {
    if !current.is_empty() {
        words.push(current.to_lowercase());
        current.clear();
    }
}

pub fn convert_case(input: &str, style: CaseStyle) -> String {
    let words = split_words(input);
    match style {
        CaseStyle::Snake => words.join("_"),
        CaseStyle::Kebab => words.join("-"),
        CaseStyle::Camel => words
            .iter()
            .enumerate()
            .map(|(index, word)| {
                if index == 0 {
                    word.clone()
                } else {
                    capitalize(word)
                }
            })
            .collect(),
        CaseStyle::Pascal => words.iter().map(|word| capitalize(word)).collect(),
    }
}

fn capitalize(word: &str) -> String {
    let mut chars = word.chars();
    match chars.next() {
        Some(first) => format!("{}{}", first.to_uppercase(), chars.as_str()),
        None => String::new(),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct TextStats {
    pub lines: usize,
    pub words: usize,
    pub chars: usize,
    pub bytes: usize,
}

pub fn stats(input: &str) -> TextStats {
    TextStats {
        lines: input.lines().count(),
        words: input.split_whitespace().count(),
        chars: input.chars().count(),
        bytes: input.len(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_camel_and_separators() {
        assert_eq!(split_words("HelloWorld"), ["hello", "world"]);
        assert_eq!(
            split_words("hello_world-again"),
            ["hello", "world", "again"]
        );
        assert_eq!(split_words("HTTPServer"), ["http", "server"]);
        assert_eq!(split_words(""), Vec::<String>::new());
    }

    #[test]
    fn converts_all_styles() {
        assert_eq!(convert_case("HelloWorld", CaseStyle::Snake), "hello_world");
        assert_eq!(convert_case("hello_world", CaseStyle::Camel), "helloWorld");
        assert_eq!(convert_case("hello-world", CaseStyle::Pascal), "HelloWorld");
        assert_eq!(convert_case("HelloWorld", CaseStyle::Kebab), "hello-world");
    }

    #[test]
    fn counts_text() {
        let stats = stats("hello world\n第二行\n");
        assert_eq!(stats.lines, 2);
        assert_eq!(stats.words, 3);
        assert_eq!(stats.bytes, "hello world\n第二行\n".len());
    }
}
