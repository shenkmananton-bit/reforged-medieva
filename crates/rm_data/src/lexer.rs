//! Первый проход: текст файла → список строк «ключ значение».
//!
//! Формат файлов Medieval II построчный. В каждой значимой строке первое
//! слово — ключ, всё остальное — значение. Всё после `;` — комментарий.
//! Пустые строки и строки из одних комментариев пропускаются.

/// Ключи, которые в оригинальном формате состоят из двух слов.
/// Например, `banner faction main_infantry` — ключ `banner faction`,
/// значение `main_infantry`.
const TWO_WORD_KEYS: &[&str] = &["banner", "era"];

/// Одна значимая строка файла.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    /// Номер строки в файле, начиная с 1.
    pub number: usize,
    /// Ключ (первое слово, либо два слова для `banner …` и `era …`).
    pub key: String,
    /// Всё, что после ключа, без комментария и без пробелов по краям.
    pub value: String,
}

/// Разбивает текст на значимые строки.
pub fn tokenize(text: &str) -> Vec<Line> {
    // Файлы, сохранённые «Блокнотом» Windows, иногда начинаются с BOM.
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut out = Vec::new();
    for (index, raw) in text.lines().enumerate() {
        let content = match raw.find(';') {
            Some(pos) => &raw[..pos],
            None => raw,
        };
        let content = content.trim();
        if content.is_empty() {
            continue;
        }
        let (key, rest) = split_word(content);
        if TWO_WORD_KEYS.contains(&key) && !rest.is_empty() {
            let (second, after) = split_word(rest);
            // Ключ хранится с одним пробелом, даже если в файле их было несколько.
            let joined = format!("{key} {second}");
            out.push(Line {
                number: index + 1,
                key: joined,
                value: after.to_string(),
            });
            continue;
        }
        out.push(Line {
            number: index + 1,
            key: key.to_string(),
            value: rest.to_string(),
        });
    }
    out
}

/// Отделяет первое слово от остатка строки.
fn split_word(s: &str) -> (&str, &str) {
    match s.find(char::is_whitespace) {
        Some(pos) => (&s[..pos], s[pos..].trim_start()),
        None => (s, ""),
    }
}

/// Делит значение по запятым: `"a, b ,c"` → `["a", "b", "c"]`.
pub fn split_list(value: &str) -> Vec<&str> {
    if value.trim().is_empty() {
        return Vec::new();
    }
    value.split(',').map(str::trim).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_comments_and_blank_lines() {
        let lines = tokenize("; заголовок\n\ntype   Spearmen ; копейщики\n");
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].number, 3);
        assert_eq!(lines[0].key, "type");
        assert_eq!(lines[0].value, "Spearmen");
    }

    #[test]
    fn two_word_keys() {
        let lines = tokenize("banner  faction   main_infantry\nera 0  northmark");
        assert_eq!(lines[0].key, "banner faction");
        assert_eq!(lines[0].value, "main_infantry");
        assert_eq!(lines[1].key, "era 0");
        assert_eq!(lines[1].value, "northmark");
    }

    #[test]
    fn bom_and_tabs() {
        let lines = tokenize("\u{feff}faction\t\tnorthmark\r\n");
        assert_eq!(lines[0].key, "faction");
        assert_eq!(lines[0].value, "northmark");
    }

    #[test]
    fn list_split() {
        assert_eq!(split_list("a, b ,c"), vec!["a", "b", "c"]);
        assert!(split_list("  ").is_empty());
    }
}
