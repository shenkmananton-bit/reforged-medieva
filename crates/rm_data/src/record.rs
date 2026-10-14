//! Второй проход: строки → записи (юнит, фракция) и разбор значений.
//!
//! Запись начинается со строки с «открывающим» ключом (`type` для юнитов,
//! `faction` для фракций) и тянется до следующего такого ключа.
//! Внутри записи поля достаются по имени; что осталось невостребованным,
//! попадает в [`ExtraField`] — так неизвестные поля модов не теряются.

use std::str::FromStr;

use crate::error::Diag;
use crate::lexer::{Line, split_list};

/// Поле, которое парсер не знает. Сохраняется как есть, чтобы
/// инструменты могли о нём предупредить, а будущий код — прочитать.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtraField {
    pub line: usize,
    pub key: String,
    pub value: String,
}

/// Сырая запись: имя и её строки.
pub(crate) struct Record {
    pub start_line: usize,
    pub name: String,
    lines: Vec<Line>,
}

/// Группирует строки в записи по открывающему ключу `start_key`.
pub(crate) fn group(lines: Vec<Line>, start_key: &str, diag: &mut Diag) -> Vec<Record> {
    let mut records: Vec<Record> = Vec::new();
    for line in lines {
        if line.key == start_key {
            if line.value.is_empty() {
                diag.error(line.number, format!("после `{start_key}` должно идти имя"));
            }
            records.push(Record {
                start_line: line.number,
                name: line.value,
                lines: Vec::new(),
            });
        } else if let Some(current) = records.last_mut() {
            current.lines.push(line);
        } else {
            diag.error(
                line.number,
                format!(
                    "строка `{}` стоит до первой записи; файл должен начинаться с `{start_key}`",
                    line.key
                ),
            );
        }
    }
    records
}

impl Record {
    /// Забирает единственное поле `key`. Повтор поля — ошибка.
    pub fn take(&mut self, key: &str, diag: &mut Diag) -> Option<Line> {
        let mut found = self.take_all(key).into_iter();
        let first = found.next()?;
        for dup in found {
            diag.error(
                dup.number,
                format!(
                    "поле `{key}` повторяется в записи `{}` (первое на строке {})",
                    self.name, first.number
                ),
            );
        }
        Some(first)
    }

    /// Забирает обязательное поле; если его нет, пишет ошибку.
    pub fn require(&mut self, key: &str, diag: &mut Diag) -> Option<Line> {
        let line = self.take(key, diag);
        if line.is_none() {
            diag.error(
                self.start_line,
                format!("в записи `{}` нет обязательного поля `{key}`", self.name),
            );
        }
        line
    }

    /// Забирает все вхождения поля (для полей, которые могут повторяться, как `officer`).
    pub fn take_all(&mut self, key: &str) -> Vec<Line> {
        let (taken, rest) = std::mem::take(&mut self.lines)
            .into_iter()
            .partition(|l| l.key == key);
        self.lines = rest;
        taken
    }

    /// Всё, что не было забрано, — неизвестные поля.
    pub fn into_extra(self) -> Vec<ExtraField> {
        self.lines
            .into_iter()
            .map(|l| ExtraField {
                line: l.number,
                key: l.key,
                value: l.value,
            })
            .collect()
    }
}

/// Значения одной строки, разделённые запятыми, с удобным разбором по позициям.
pub(crate) struct Values<'l> {
    line: &'l Line,
    parts: Vec<&'l str>,
}

impl<'l> Values<'l> {
    /// Делит строку по запятым и проверяет, что значений не меньше `min`.
    /// `layout` — подсказка о формате для текста ошибки.
    pub fn new(line: &'l Line, min: usize, layout: &str, diag: &mut Diag) -> Option<Self> {
        let parts = split_list(&line.value);
        if parts.len() < min {
            diag.error(
                line.number,
                format!(
                    "в `{}` ожидается не меньше {min} значений через запятую ({layout}), а найдено {}",
                    line.key,
                    parts.len()
                ),
            );
            return None;
        }
        Some(Self { line, parts })
    }

    pub fn len(&self) -> usize {
        self.parts.len()
    }

    pub fn str(&self, i: usize) -> String {
        self.parts[i].to_string()
    }

    /// Значения с позиции `from` до конца.
    pub fn rest(&self, from: usize) -> Vec<String> {
        self.parts
            .iter()
            .skip(from)
            .map(|s| s.to_string())
            .collect()
    }

    /// Число на позиции `i`; `what` — что это за число, для текста ошибки.
    pub fn num<T: FromStr>(&self, i: usize, what: &str, diag: &mut Diag) -> Option<T> {
        parse_num(self.line, self.parts[i], what, diag)
    }
}

pub(crate) fn parse_num<T: FromStr>(
    line: &Line,
    text: &str,
    what: &str,
    diag: &mut Diag,
) -> Option<T> {
    match text.trim().parse() {
        Ok(v) => Some(v),
        Err(_) => {
            diag.error(
                line.number,
                format!(
                    "в `{}`: {what} должно быть числом, а написано `{text}`",
                    line.key
                ),
            );
            None
        }
    }
}

pub(crate) fn parse_bool(line: &Line, text: &str, what: &str, diag: &mut Diag) -> Option<bool> {
    match text.trim() {
        "yes" => Some(true),
        "no" => Some(false),
        other => {
            diag.error(
                line.number,
                format!(
                    "в `{}`: {what} должно быть `yes` или `no`, а написано `{other}`",
                    line.key
                ),
            );
            None
        }
    }
}

/// Значение строки, которое должно быть одним словом (например, `culture`).
pub(crate) fn single_word(line: &Line, diag: &mut Diag) -> Option<String> {
    let v = line.value.trim();
    if v.is_empty() {
        diag.error(line.number, format!("у поля `{}` нет значения", line.key));
        return None;
    }
    if v.contains(char::is_whitespace) {
        diag.error(
            line.number,
            format!(
                "у поля `{}` должно быть одно слово, а написано `{v}`",
                line.key
            ),
        );
        return None;
    }
    Some(v.to_string())
}

/// Значение строки целиком (например, путь к файлу).
pub(crate) fn text(line: &Line, diag: &mut Diag) -> Option<String> {
    if line.value.is_empty() {
        diag.error(line.number, format!("у поля `{}` нет значения", line.key));
        return None;
    }
    Some(line.value.clone())
}

/// Необязательное поле, разбор которого мог провалиться:
/// `None` — поля нет (это нормально), `Some(None)` — поле есть, но с ошибкой.
pub(crate) trait TransposeInner<T> {
    /// Превращает `Option<Option<T>>` в `Option<Option<T>>` «наоборот»:
    /// внешний `None` значит «была ошибка», внутренний — «поля нет».
    fn transpose_inner(self) -> Option<Option<T>>;
}

impl<T> TransposeInner<T> for Option<Option<T>> {
    fn transpose_inner(self) -> Option<Option<T>> {
        match self {
            None => Some(None),
            Some(Some(v)) => Some(Some(v)),
            Some(None) => None,
        }
    }
}

pub(crate) fn pick<T: Copy>(value: &str, table: &[(&str, T)]) -> Option<T> {
    table
        .iter()
        .find(|(name, _)| *name == value)
        .map(|(_, v)| *v)
}

pub(crate) fn names<T>(table: &[(&str, T)]) -> String {
    table.iter().map(|(n, _)| *n).collect::<Vec<_>>().join(", ")
}
