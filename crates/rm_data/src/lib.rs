//! # rm_data — чтение текстовых файлов данных
//!
//! Игра, как и Medieval II, хранит статы юнитов, фракции и прочие данные
//! в обычных текстовых файлах в папке `data/`. Их может править любой
//! моддер в блокноте. Этот крейт превращает такие файлы в обычные
//! Rust-структуры и объясняет ошибки в виде `файл:строка: причина`.
//!
//! Разбор идёт в два прохода:
//! 1. [`lexer`] режет текст на строки «ключ значение», выкидывая комментарии;
//! 2. парсеры конкретных файлов ([`unit`], [`faction`]) собирают из строк записи
//!    и проверяют значения.
//!
//! Подробное описание — в `docs/data/`, руководство для моддеров — в `docs/modding/`.

pub mod campaign;
pub mod error;
pub mod faction;
pub mod lexer;
mod record;
pub mod region;
pub mod resource;
pub mod strat;
pub mod unit;

use std::path::Path;

pub use campaign::{CampaignData, CampaignRules, RegionLink, SettlementLevel};
pub use error::{DataError, DataErrors};
pub use faction::{FACTIONS_FILE, Faction, Rgb, load_factions, parse_factions};
pub use record::ExtraField;
pub use region::Region;
pub use resource::Resource;
pub use strat::{Date, Season, Strat, StratCharacter, StratFaction, StratSettlement, StratUnit};
pub use unit::{UNITS_FILE, Unit, UnitCategory, UnitClass, load_units, parse_units};

/// Все данные, загруженные из одной папки `data/`.
#[derive(Debug, Clone)]
pub struct GameData {
    pub units: Vec<Unit>,
    pub factions: Vec<Faction>,
}

impl GameData {
    /// Загружает `export_descr_unit.txt` и `descr_sm_factions.txt` из папки `dir`
    /// и проверяет ссылки между ними. Возвращает все найденные ошибки разом.
    pub fn load_dir(dir: &Path) -> Result<Self, DataErrors> {
        let mut errors = Vec::new();
        let units = load_units(&dir.join(UNITS_FILE))
            .map_err(|e| errors.extend(e.0))
            .ok();
        let factions = load_factions(&dir.join(FACTIONS_FILE))
            .map_err(|e| errors.extend(e.0))
            .ok();
        if let (Some(units), Some(factions)) = (units, factions) {
            let data = GameData { units, factions };
            errors.extend(data.check_references(&dir.join(UNITS_FILE)));
            if errors.is_empty() {
                return Ok(data);
            }
        }
        Err(DataErrors(errors))
    }

    /// Проверяет, что каждая фракция в `ownership` юнитов объявлена в файле фракций.
    /// `units_file` — путь к файлу юнитов для сообщений.
    pub fn check_references(&self, units_file: &Path) -> Vec<DataError> {
        let mut errors = Vec::new();
        for unit in &self.units {
            for owner in &unit.ownership {
                // `slave` — особое имя мятежников, как в оригинале; объявлять его не обязательно.
                if owner != "slave" && self.faction(owner).is_none() {
                    errors.push(DataError::new(
                        units_file,
                        unit.line,
                        format!("юнит `{}`: фракция `{owner}` из `ownership` не объявлена в {FACTIONS_FILE}", unit.name),
                    ));
                }
            }
        }
        errors
    }

    pub fn unit(&self, name: &str) -> Option<&Unit> {
        self.units.iter().find(|u| u.name == name)
    }

    pub fn faction(&self, name: &str) -> Option<&Faction> {
        self.factions.iter().find(|f| f.name == name)
    }
}

/// Читает файл целиком; ошибку ввода-вывода превращает в понятную [`DataError`].
///
/// Как и в оригинале, понимает две кодировки: UTF-8 и Windows-1252
/// (в ней сохранены многие старые моды). Если байты не являются
/// правильным UTF-8, файл читается как Windows-1252.
pub(crate) fn read_file(path: &Path) -> Result<String, DataError> {
    let bytes = std::fs::read(path).map_err(|e| {
        let reason = match e.kind() {
            std::io::ErrorKind::NotFound => "файл не найден".to_string(),
            _ => format!("не удалось прочитать файл: {e}"),
        };
        DataError::new(path, 0, reason)
    })?;
    Ok(decode_text(bytes))
}

/// UTF-8, если получается, иначе Windows-1252.
pub fn decode_text(bytes: Vec<u8>) -> String {
    match String::from_utf8(bytes) {
        Ok(s) => s,
        Err(e) => e.into_bytes().iter().map(|&b| windows_1252(b)).collect(),
    }
}

/// Символ Windows-1252 для байта. Совпадает с Latin-1, кроме диапазона 0x80–0x9F.
fn windows_1252(b: u8) -> char {
    const HIGH: [char; 32] = [
        '€', '\u{81}', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', '\u{8d}', 'Ž',
        '\u{8f}', '\u{90}', '‘', '’', '“', '”', '•', '–', '—', '˜', '™', 'š', '›', 'œ', '\u{9d}',
        'ž', 'Ÿ',
    ];
    match b {
        0x80..=0x9f => HIGH[(b - 0x80) as usize],
        _ => b as char,
    }
}

#[cfg(test)]
mod tests {
    use super::decode_text;

    #[test]
    fn decodes_utf8_and_windows_1252() {
        assert_eq!(decode_text("Ополчение".as_bytes().to_vec()), "Ополчение");
        // «Café — ok» в Windows-1252: é = 0xE9, длинное тире = 0x97.
        assert_eq!(decode_text(b"Caf\xe9 \x97 ok".to_vec()), "Café — ok");
    }
}
