//! Ресурсы карты кампании: формат `descr_sm_resources.txt`.
//!
//! Каждый ресурс начинается со строки `type <имя>`:
//!
//! ```text
//! type          wine
//! trade_value   8
//! item          models_strat/resource_wine.cas
//! icon          ui/resources/resource_wine.tga
//! ```

use std::path::Path;

use crate::error::{DataErrors, Diag};
use crate::lexer::tokenize;
use crate::record::{ExtraField, Record, group, parse_num, text};

/// Путь к файлу ресурсов внутри папки `data/`.
pub const RESOURCES_FILE: &str = "descr_sm_resources.txt";

/// Один ресурс из `descr_sm_resources.txt`.
#[derive(Debug, Clone, PartialEq)]
pub struct Resource {
    /// Внутреннее имя (`type`). На него ссылаются регионы.
    pub name: String,
    /// Торговая ценность: сколько денег ресурс приносит региону за ход.
    pub trade_value: u32,
    /// Модель на карте (`item`).
    pub item: Option<String>,
    /// Иконка в интерфейсе (`icon`).
    pub icon: Option<String>,
    /// Добывается ли шахтой (`has_mine`).
    pub has_mine: bool,
    /// Строка, на которой начинается ресурс.
    pub line: usize,
    /// Поля, которые парсер пока не знает.
    pub extra: Vec<ExtraField>,
}

/// Разбирает текст `descr_sm_resources.txt`.
pub fn parse_resources(source: &str, file: &Path) -> Result<Vec<Resource>, DataErrors> {
    let mut diag = Diag::new(file);
    let records = group(tokenize(source), "type", &mut diag);
    let mut out: Vec<Resource> = Vec::new();
    for record in records {
        if let Some(r) = parse_resource(record, &mut diag) {
            if let Some(prev) = out.iter().find(|x| x.name == r.name) {
                diag.error(
                    r.line,
                    format!("ресурс `{}` уже объявлен на строке {}", r.name, prev.line),
                );
                continue;
            }
            out.push(r);
        }
    }
    diag.finish(out)
}

/// Читает и разбирает файл ресурсов с диска.
pub fn load_resources(path: &Path) -> Result<Vec<Resource>, DataErrors> {
    let source = crate::read_file(path)?;
    parse_resources(&source, path)
}

fn parse_resource(mut r: Record, diag: &mut Diag) -> Option<Resource> {
    let trade_value = r
        .require("trade_value", diag)
        .and_then(|l| parse_num::<u32>(&l, &l.value, "торговая ценность", diag));
    let item = r.take("item", diag).and_then(|l| text(&l, diag));
    let icon = r.take("icon", diag).and_then(|l| text(&l, diag));
    let has_mine = r.take("has_mine", diag).is_some();
    let name = r.name.clone();
    let line = r.start_line;
    let extra = r.into_extra();
    Some(Resource {
        name,
        trade_value: trade_value?,
        item,
        icon,
        has_mine,
        line,
        extra,
    })
}
