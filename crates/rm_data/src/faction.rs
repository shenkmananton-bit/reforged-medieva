//! Фракции: формат `descr_sm_factions.txt`.
//!
//! Каждая фракция начинается со строки `faction <имя>`.

use std::path::Path;

use crate::error::{DataErrors, Diag};
use crate::lexer::{Line, split_list, tokenize};
use crate::record::{
    ExtraField, Record, TransposeInner, group, parse_bool, parse_num, single_word, text,
};

/// Имя файла фракций в папке `data/`.
pub const FACTIONS_FILE: &str = "descr_sm_factions.txt";

/// Цвет в формате `red 200, green 30, blue 30`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

/// Одна фракция из `descr_sm_factions.txt`.
#[derive(Debug, Clone, PartialEq)]
pub struct Faction {
    /// Внутреннее имя (`faction`). На него ссылается `ownership` у юнитов.
    pub name: String,
    pub culture: String,
    pub religion: String,
    pub symbol: Option<String>,
    pub rebel_symbol: Option<String>,
    pub primary_colour: Rgb,
    pub secondary_colour: Rgb,
    pub loading_logo: Option<String>,
    pub standard_index: Option<u32>,
    pub logon_logo: Option<String>,
    /// Доступна ли в своей битве. По умолчанию `yes`.
    pub custom_battle_availability: bool,
    /// Умеет ли делать подкопы. По умолчанию `no`.
    pub can_sap: bool,
    /// Предпочитает ли ИИ морские вторжения. По умолчанию `no`.
    pub prefers_naval_invasions: bool,
    /// Есть ли принцессы. По умолчанию `no`.
    pub can_have_princess: bool,
    /// Есть ли семейное древо. По умолчанию `no`.
    pub has_family_tree: bool,
    /// Строка, на которой начинается фракция.
    pub line: usize,
    /// Поля, которые парсер пока не знает.
    pub extra: Vec<ExtraField>,
}

/// Разбирает текст `descr_sm_factions.txt`.
pub fn parse_factions(source: &str, file: &Path) -> Result<Vec<Faction>, DataErrors> {
    let mut diag = Diag::new(file);
    let records = group(tokenize(source), "faction", &mut diag);
    let mut factions: Vec<Faction> = Vec::new();
    for record in records {
        if let Some(f) = parse_faction(record, &mut diag) {
            if let Some(prev) = factions.iter().find(|x| x.name == f.name) {
                diag.error(
                    f.line,
                    format!("фракция `{}` уже объявлена на строке {}", f.name, prev.line),
                );
                continue;
            }
            factions.push(f);
        }
    }
    diag.finish(factions)
}

/// Читает и разбирает файл фракций с диска.
pub fn load_factions(path: &Path) -> Result<Vec<Faction>, DataErrors> {
    let source = crate::read_file(path)?;
    parse_factions(&source, path)
}

fn parse_faction(mut r: Record, diag: &mut Diag) -> Option<Faction> {
    let culture = r
        .require("culture", diag)
        .and_then(|l| single_word(&l, diag));
    let religion = r
        .require("religion", diag)
        .and_then(|l| single_word(&l, diag));
    let symbol = r.take("symbol", diag).and_then(|l| text(&l, diag));
    let rebel_symbol = r.take("rebel_symbol", diag).and_then(|l| text(&l, diag));
    let primary = r
        .require("primary_colour", diag)
        .and_then(|l| colour(&l, diag));
    let secondary = r
        .require("secondary_colour", diag)
        .and_then(|l| colour(&l, diag));
    let loading_logo = r.take("loading_logo", diag).and_then(|l| text(&l, diag));
    let standard_index = r
        .take("standard_index", diag)
        .map(|l| parse_num(&l, &l.value, "номер знамени", diag))
        .transpose_inner();
    let logon_logo = r.take("logon_logo", diag).and_then(|l| text(&l, diag));
    let mut flag = |key: &str, default: bool, what: &str, diag: &mut Diag| -> Option<bool> {
        match r.take(key, diag) {
            Some(l) => parse_bool(&l, &l.value, what, diag),
            None => Some(default),
        }
    };
    let custom_battle = flag(
        "custom_battle_availability",
        true,
        "доступность в своей битве",
        diag,
    );
    let can_sap = flag("can_sap", false, "умение делать подкопы", diag);
    let naval = flag(
        "prefers_naval_invasions",
        false,
        "предпочтение морских вторжений",
        diag,
    );
    let princess = flag("can_have_princess", false, "наличие принцесс", diag);
    let family_tree = flag("has_family_tree", false, "наличие семейного древа", diag);

    let name = r.name.clone();
    let line = r.start_line;
    let extra = r.into_extra();
    Some(Faction {
        name,
        culture: culture?,
        religion: religion?,
        symbol,
        rebel_symbol,
        primary_colour: primary?,
        secondary_colour: secondary?,
        loading_logo,
        standard_index: standard_index?,
        logon_logo,
        custom_battle_availability: custom_battle?,
        can_sap: can_sap?,
        prefers_naval_invasions: naval?,
        can_have_princess: princess?,
        has_family_tree: family_tree?,
        line,
        extra,
    })
}

/// `red 200, green 30, blue 30` → [`Rgb`].
fn colour(l: &Line, diag: &mut Diag) -> Option<Rgb> {
    let parts = split_list(&l.value);
    let expected = ["red", "green", "blue"];
    if parts.len() != 3 {
        diag.error(
            l.number,
            format!("цвет `{}` записывается как `red 0, green 0, blue 0`", l.key),
        );
        return None;
    }
    let mut out = [0u8; 3];
    let mut ok = true;
    for (i, part) in parts.iter().enumerate() {
        let mut words = part.split_whitespace();
        let (Some(channel), Some(value), None) = (words.next(), words.next(), words.next()) else {
            diag.error(
                l.number,
                format!(
                    "в `{}` часть `{part}` должна быть вида `{} <0-255>`",
                    l.key, expected[i]
                ),
            );
            ok = false;
            continue;
        };
        if channel != expected[i] {
            diag.error(
                l.number,
                format!(
                    "в `{}` на месте {} ожидается `{}`, а написано `{channel}`",
                    l.key,
                    i + 1,
                    expected[i]
                ),
            );
            ok = false;
            continue;
        }
        match parse_num::<u8>(l, value, &format!("канал {channel} (0–255)"), diag) {
            Some(v) => out[i] = v,
            None => ok = false,
        }
    }
    ok.then_some(Rgb {
        r: out[0],
        g: out[1],
        b: out[2],
    })
}
