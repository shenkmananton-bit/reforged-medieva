//! Ошибки разбора: каждая знает файл, строку и причину.

use std::fmt;
use std::path::{Path, PathBuf};

/// Одна ошибка в файле данных.
///
/// Печатается в привычном для редакторов виде `файл:строка: причина`,
/// поэтому по ней можно сразу прыгнуть к нужному месту.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataError {
    /// Файл, в котором найдена ошибка.
    pub file: PathBuf,
    /// Номер строки, начиная с 1. Ноль значит «ошибка относится ко всему файлу»
    /// (например, файл не удалось открыть).
    pub line: usize,
    /// Человекочитаемая причина.
    pub message: String,
}

impl DataError {
    pub fn new(file: impl Into<PathBuf>, line: usize, message: impl Into<String>) -> Self {
        Self {
            file: file.into(),
            line,
            message: message.into(),
        }
    }
}

impl fmt::Display for DataError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.line == 0 {
            write!(f, "{}: {}", self.file.display(), self.message)
        } else {
            write!(f, "{}:{}: {}", self.file.display(), self.line, self.message)
        }
    }
}

impl std::error::Error for DataError {}

/// Набор ошибок. Парсер не останавливается на первой ошибке,
/// а собирает все, чтобы моддер мог исправить их за один проход.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DataErrors(pub Vec<DataError>);

impl DataErrors {
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn iter(&self) -> impl Iterator<Item = &DataError> {
        self.0.iter()
    }
}

impl fmt::Display for DataErrors {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, e) in self.0.iter().enumerate() {
            if i > 0 {
                writeln!(f)?;
            }
            write!(f, "{e}")?;
        }
        Ok(())
    }
}

impl std::error::Error for DataErrors {}

impl From<DataError> for DataErrors {
    fn from(e: DataError) -> Self {
        Self(vec![e])
    }
}

/// Сборщик ошибок для одного файла. Используется внутри парсеров.
pub(crate) struct Diag<'a> {
    file: &'a Path,
    pub(crate) errors: Vec<DataError>,
}

impl<'a> Diag<'a> {
    pub(crate) fn new(file: &'a Path) -> Self {
        Self {
            file,
            errors: Vec::new(),
        }
    }

    pub(crate) fn error(&mut self, line: usize, message: impl Into<String>) {
        self.errors.push(DataError::new(self.file, line, message));
    }

    /// Возвращает `value`, если ошибок не было, иначе все накопленные ошибки.
    pub(crate) fn finish<T>(self, value: T) -> Result<T, DataErrors> {
        if self.errors.is_empty() {
            Ok(value)
        } else {
            Err(DataErrors(self.errors))
        }
    }
}
