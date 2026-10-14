//! Регионы карты кампании: формат `descr_regions.txt`.
//!
//! В отличие от юнитов и фракций, у региона нет ключей: это девять строк
//! подряд, смысл каждой задаётся её позицией (как в оригинале):
//!
//! ```text
//! Ostmark                      ; 1. имя региона
//!     Ostburg                  ; 2. имя поселения
//!     northmark                ; 3. фракция-основатель (для культуры построек)
//!     Northern_Rebels          ; 4. тип мятежников
//!     40 70 160                ; 5. цвет региона на карте регионов (R G B)
//!     wine, timber             ; 6. ресурсы (или `none`)
//!     5                        ; 7. очки триумфа
//!     3                        ; 8. уровень плодородия (фермерство)
//!     religions { catholic 90 pagan 10 }  ; 9. доли религий
//! ```
//!
//! Строка `religions { … }` закрывает регион, следующая строка начинает новый.

use std::path::Path;

use crate::error::{DataErrors, Diag};
use crate::faction::Rgb;
use crate::lexer::{Line, split_list, tokenize};
use crate::record::parse_num;

/// Путь к файлу регионов внутри папки `data/` (как в оригинале).
pub const REGIONS_FILE: &str = "world/maps/base/descr_regions.txt";

/// Один регион из `descr_regions.txt`.
#[derive(Debug, Clone, PartialEq)]
pub struct Region {
    pub name: String,
    /// Имя поселения (столицы региона).
    pub settlement: String,
    /// Фракция-основатель: определяет культуру построек поселения.
    pub creator: String,
    /// Тип мятежников, которые появятся при восстании.
    pub rebels: String,
    /// Цвет региона на карте регионов `map_regions.tga`.
    pub colour: Rgb,
    /// Ресурсы региона; ссылаются на `descr_sm_resources.txt`.
    pub resources: Vec<String>,
    /// Очки триумфа (вес региона для условий победы).
    pub triumph_value: u32,
    /// Уровень плодородия: чем выше, тем больше доход с ферм и рост населения.
    pub farming_level: u32,
    /// Религии и их доли в процентах.
    pub religions: Vec<(String, u32)>,
    /// Строка, на которой начинается регион.
    pub line: usize,
}

/// Разбирает текст `descr_regions.txt`.
pub fn parse_regions(source: &str, file: &Path) -> Result<Vec<Region>, DataErrors> {
    let mut diag = Diag::new(file);
    let mut regions: Vec<Region> = Vec::new();
    let mut block: Vec<Line> = Vec::new();
    for line in tokenize(source) {
        let closes = line.key == "religions";
        block.push(line);
        if closes {
            if let Some(region) = parse_region(&block, &mut diag) {
                if let Some(prev) = regions.iter().find(|r| r.name == region.name) {
                    diag.error(
                        region.line,
                        format!(
                            "регион `{}` уже объявлен на строке {}",
                            region.name, prev.line
                        ),
                    );
                } else {
                    regions.push(region);
                }
            }
            block.clear();
        }
    }
    if let Some(first) = block.first() {
        diag.error(
            first.number,
            "регион не закончен: последней строкой региона должна быть `religions { … }`",
        );
    }
    diag.finish(regions)
}

/// Читает и разбирает файл регионов с диска.
pub fn load_regions(path: &Path) -> Result<Vec<Region>, DataErrors> {
    let source = crate::read_file(path)?;
    parse_regions(&source, path)
}

/// Полный текст строки: ключ и значение снова вместе.
fn full(l: &Line) -> String {
    if l.value.is_empty() {
        l.key.clone()
    } else {
        format!("{} {}", l.key, l.value)
    }
}

fn parse_region(block: &[Line], diag: &mut Diag) -> Option<Region> {
    const LAYOUT: &str = "имя региона, поселение, фракция-основатель, мятежники, цвет, \
                          ресурсы, триумф, плодородие, religions { … }";
    let start = block[0].number;
    if block.len() != 9 {
        diag.error(
            start,
            format!(
                "у региона должно быть ровно 9 строк ({LAYOUT}), а найдено {}",
                block.len()
            ),
        );
        return None;
    }
    let word = |l: &Line, what: &str, diag: &mut Diag| -> Option<String> {
        if l.value.is_empty() {
            Some(l.key.clone())
        } else {
            diag.error(
                l.number,
                format!("{what} должно быть одним словом, а написано `{}`", full(l)),
            );
            None
        }
    };
    let name = word(&block[0], "имя региона", diag);
    let settlement = word(&block[1], "имя поселения", diag);
    let creator = word(&block[2], "фракция-основатель", diag);
    let rebels = word(&block[3], "тип мятежников", diag);

    let colour = {
        let l = &block[4];
        let text = full(l);
        let parts: Vec<&str> = text.split_whitespace().collect();
        if parts.len() == 3 {
            let r = parse_num::<u8>(l, parts[0], "красный канал (0–255)", diag);
            let g = parse_num::<u8>(l, parts[1], "зелёный канал (0–255)", diag);
            let b = parse_num::<u8>(l, parts[2], "синий канал (0–255)", diag);
            match (r, g, b) {
                (Some(r), Some(g), Some(b)) => Some(Rgb { r, g, b }),
                _ => None,
            }
        } else {
            diag.error(
                l.number,
                format!("цвет региона записывается тремя числами `R G B`, а написано `{text}`"),
            );
            None
        }
    };

    let resources = {
        let text = full(&block[5]);
        if text.trim() == "none" {
            Vec::new()
        } else {
            split_list(&text).into_iter().map(str::to_string).collect()
        }
    };
    let triumph_value = parse_num::<u32>(&block[6], &full(&block[6]), "очки триумфа", diag);
    let farming_level = parse_num::<u32>(&block[7], &full(&block[7]), "уровень плодородия", diag);
    let religions = religions(&block[8], diag);

    Some(Region {
        name: name?,
        settlement: settlement?,
        creator: creator?,
        rebels: rebels?,
        colour: colour?,
        resources,
        triumph_value: triumph_value?,
        farming_level: farming_level?,
        religions: religions?,
        line: start,
    })
}

/// `religions { catholic 90 pagan 10 }` → `[("catholic", 90), ("pagan", 10)]`.
fn religions(l: &Line, diag: &mut Diag) -> Option<Vec<(String, u32)>> {
    let inner = l
        .value
        .trim()
        .strip_prefix('{')
        .and_then(|s| s.strip_suffix('}'));
    let Some(inner) = inner else {
        diag.error(
            l.number,
            "религии записываются как `religions { catholic 90 pagan 10 }`",
        );
        return None;
    };
    let words: Vec<&str> = inner.split_whitespace().collect();
    if !words.len().is_multiple_of(2) {
        diag.error(
            l.number,
            "в `religions` после каждой религии должна стоять её доля в процентах",
        );
        return None;
    }
    let mut out = Vec::new();
    let mut ok = true;
    for pair in words.chunks(2) {
        match parse_num::<u32>(l, pair[1], &format!("доля религии {}", pair[0]), diag) {
            Some(share) => out.push((pair[0].to_string(), share)),
            None => ok = false,
        }
    }
    ok.then_some(out)
}
