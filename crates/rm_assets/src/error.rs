//! Ошибки конвейера ассетов.

use std::fmt;

/// Что пошло не так при сборке, записи или чтении ассетов.
#[derive(Debug)]
pub enum AssetError {
    /// Файл TGA повреждён или в неподдерживаемом варианте формата.
    Tga(String),
    /// Пакет `.rmpak` повреждён, другой версии или в нём нет нужной записи.
    Pack(String),
    /// Текстуры не помещаются в атлас или повторяется имя.
    Atlas(String),
    /// Модель или LOD-цепочка нарушает правила (см. `docs/assets/lod.md`).
    Invalid(String),
    /// Ошибка чтения или записи файла.
    Io(std::io::Error),
}

impl fmt::Display for AssetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AssetError::Tga(m) => write!(f, "TGA: {m}"),
            AssetError::Pack(m) => write!(f, "пакет .rmpak: {m}"),
            AssetError::Atlas(m) => write!(f, "атлас: {m}"),
            AssetError::Invalid(m) => write!(f, "ассет: {m}"),
            AssetError::Io(e) => write!(f, "ввод-вывод: {e}"),
        }
    }
}

impl std::error::Error for AssetError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            AssetError::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for AssetError {
    fn from(e: std::io::Error) -> Self {
        AssetError::Io(e)
    }
}

pub type Result<T> = std::result::Result<T, AssetError>;
