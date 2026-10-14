//! Стартовая расстановка кампании: формат `descr_strat.txt`.
//!
//! Поддержано подмножество оригинального формата, нужное базовым правилам
//! кампании:
//!
//! ```text
//! campaign        imperial_campaign
//! playable
//!     northmark
//! end
//! nonplayable
//!     slave
//! end
//! start_date      1080 summer
//! end_date        1200 winter
//! timescale       2.00
//!
//! faction         northmark, comfortable caution
//! ai_label        catholic
//! denari          5000
//! denari_kings_purse  1500
//! settlement
//! {
//!     level           town
//!     region          Ostmark
//!     year_founded    0
//!     population      2400
//!     plan_set        default_set
//!     faction_creator northmark
//!     building
//!     {
//!         type core_building wooden_pallisade
//!     }
//! }
//! character   Aldric, named character, male, leader, age 40, x 12, y 30
//! army
//! unit        Shield Wall Spearmen    exp 1 armour 0 weapon_lvl 0
//! ```
//!
//! Строки, которые разбор пока не понимает (`traits`, `ancillaries`,
//! `resource`, `core_attitudes`…), не теряются: они попадают в `extra`.

use std::path::Path;

use crate::error::{DataErrors, Diag};
use crate::lexer::{Line, split_list, tokenize};
use crate::record::{ExtraField, parse_num};

/// Путь к файлу расстановки внутри папки `data/` (как в оригинале).
pub const STRAT_FILE: &str = "world/maps/campaign/imperial_campaign/descr_strat.txt";

/// Время года. В Medieval II ходы чередуют лето и зиму.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Season {
    Summer,
    Winter,
}

/// Дата: год и время года.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Date {
    pub year: i32,
    pub season: Season,
}

/// Весь файл `descr_strat.txt`.
#[derive(Debug, Clone, PartialEq)]
pub struct Strat {
    /// Имя кампании (`campaign`).
    pub campaign: String,
    /// Фракции, за которые можно играть сразу.
    pub playable: Vec<String>,
    /// Фракции, которые открываются позже.
    pub unlockable: Vec<String>,
    /// Фракции только для ИИ.
    pub nonplayable: Vec<String>,
    pub start_date: Date,
    pub end_date: Date,
    /// Лет за ход, в сотых долях: `timescale 2.00` → 200.
    pub timescale_hundredths: u32,
    /// Фракции в порядке файла. Этот же порядок — порядок ходов.
    pub factions: Vec<StratFaction>,
    /// Строки верхнего уровня, которые разбор пока не знает.
    pub extra: Vec<ExtraField>,
}

/// Стартовое состояние одной фракции.
#[derive(Debug, Clone, PartialEq)]
pub struct StratFaction {
    pub name: String,
    /// Характер ИИ (`comfortable caution` после запятой), если указан.
    pub ai_personality: Option<String>,
    pub ai_label: Option<String>,
    /// Деньги в казне на старте.
    pub denari: i64,
    /// «Кошелёк короля»: деньги, которые фракция получает каждый ход сверху.
    pub kings_purse: i64,
    pub settlements: Vec<StratSettlement>,
    pub characters: Vec<StratCharacter>,
    pub line: usize,
    pub extra: Vec<ExtraField>,
}

/// Поселение из блока `settlement { … }`.
#[derive(Debug, Clone, PartialEq)]
pub struct StratSettlement {
    /// Уровень: `village`, `town`, `large_town`, `city`, `large_city`, `huge_city`.
    pub level: String,
    /// Регион из `descr_regions.txt`.
    pub region: String,
    pub year_founded: i32,
    pub population: u32,
    pub plan_set: Option<String>,
    pub faction_creator: Option<String>,
    /// Постройки (`type <цепочка> <уровень>` во вложенных блоках `building`).
    pub buildings: Vec<String>,
    pub line: usize,
}

/// Персонаж (`character`) и его армия.
#[derive(Debug, Clone, PartialEq)]
pub struct StratCharacter {
    pub name: String,
    /// Тип: `named character`, `general`, `spy`, `admiral`…
    pub kind: String,
    /// Метки из строки персонажа: `male`, `leader`, `heir`…
    pub flags: Vec<String>,
    pub age: Option<u32>,
    /// Клетка на карте кампании.
    pub x: i32,
    pub y: i32,
    /// Отряды из строк `unit` после `army`.
    pub army: Vec<StratUnit>,
    pub line: usize,
}

/// Отряд в армии персонажа.
#[derive(Debug, Clone, PartialEq)]
pub struct StratUnit {
    /// Имя из `export_descr_unit.txt` (`type`).
    pub name: String,
    pub exp: u32,
    pub armour: u32,
    pub weapon_lvl: u32,
    pub line: usize,
}

/// Строки верхнего уровня, которые никогда не относятся к фракции,
/// даже если стоят после неё.
const TOP_LEVEL: &[&str] = &[
    "resource",
    "landmark",
    "region",
    "core_attitudes",
    "faction_relationships",
    "script",
];

/// Разбирает текст `descr_strat.txt`.
pub fn parse_strat(source: &str, file: &Path) -> Result<Strat, DataErrors> {
    let mut diag = Diag::new(file);
    let lines = tokenize(source);
    let mut p = Parser {
        lines: &lines,
        pos: 0,
    };

    let mut campaign = None;
    let mut playable = Vec::new();
    let mut unlockable = Vec::new();
    let mut nonplayable = Vec::new();
    let mut start_date = None;
    let mut end_date = None;
    let mut timescale = None;
    let mut factions: Vec<StratFaction> = Vec::new();
    let mut extra = Vec::new();

    while let Some(l) = p.next() {
        match l.key.as_str() {
            "campaign" => campaign = Some(l.value.clone()),
            "playable" => playable = p.list_until_end(l, &mut diag),
            "unlockable" => unlockable = p.list_until_end(l, &mut diag),
            "nonplayable" => nonplayable = p.list_until_end(l, &mut diag),
            "start_date" => start_date = date(l, &mut diag),
            "end_date" => end_date = date(l, &mut diag),
            "timescale" => timescale = hundredths(l, &mut diag),
            "faction" => {
                let parts = split_list(&l.value);
                let name = parts.first().copied().unwrap_or_default().to_string();
                if name.is_empty() {
                    diag.error(l.number, "после `faction` должно идти имя фракции");
                }
                if let Some(prev) = factions.iter().find(|f| f.name == name) {
                    diag.error(
                        l.number,
                        format!("фракция `{name}` уже расставлена на строке {}", prev.line),
                    );
                }
                factions.push(StratFaction {
                    name,
                    ai_personality: parts.get(1).map(|s| s.to_string()),
                    ai_label: None,
                    denari: 0,
                    kings_purse: 0,
                    settlements: Vec::new(),
                    characters: Vec::new(),
                    line: l.number,
                    extra: Vec::new(),
                });
            }
            key if TOP_LEVEL.contains(&key) => extra.push(extra_field(l)),
            _ => match factions.last_mut() {
                Some(f) => faction_line(f, l, &mut p, &mut diag),
                None => extra.push(extra_field(l)),
            },
        }
    }

    let campaign = required(campaign, "campaign", &mut diag);
    let start_date = required(start_date, "start_date", &mut diag);
    let end_date = required(end_date, "end_date", &mut diag);
    let timescale = required(timescale, "timescale", &mut diag);
    let result = match (campaign, start_date, end_date, timescale) {
        (Some(campaign), Some(start_date), Some(end_date), Some(timescale_hundredths)) => {
            Some(Strat {
                campaign,
                playable,
                unlockable,
                nonplayable,
                start_date,
                end_date,
                timescale_hundredths,
                factions,
                extra,
            })
        }
        _ => None,
    };
    match result {
        Some(strat) => diag.finish(strat),
        None => Err(DataErrors(diag.errors)),
    }
}

/// Читает и разбирает файл расстановки с диска.
pub fn load_strat(path: &Path) -> Result<Strat, DataErrors> {
    let source = crate::read_file(path)?;
    parse_strat(&source, path)
}

struct Parser<'a> {
    lines: &'a [Line],
    pos: usize,
}

impl<'a> Parser<'a> {
    fn next(&mut self) -> Option<&'a Line> {
        let l = self.lines.get(self.pos)?;
        self.pos += 1;
        Some(l)
    }

    fn peek(&self) -> Option<&'a Line> {
        self.lines.get(self.pos)
    }

    /// Список имён по одному на строке до `end`.
    fn list_until_end(&mut self, opener: &Line, diag: &mut Diag) -> Vec<String> {
        let mut out = Vec::new();
        while let Some(l) = self.next() {
            if l.key == "end" {
                return out;
            }
            out.push(l.key.clone());
        }
        diag.error(
            opener.number,
            format!("список `{}` не закрыт строкой `end`", opener.key),
        );
        out
    }

    /// Строки блока `{ … }` сразу после `opener`. Вложенные блоки входят
    /// в результат вместе со своими скобками.
    fn block(&mut self, opener: &Line, diag: &mut Diag) -> Option<Vec<&'a Line>> {
        if self.peek().map(|l| l.key.as_str()) != Some("{") {
            diag.error(
                opener.number,
                format!("после `{}` должна идти строка `{{`", opener.key),
            );
            return None;
        }
        self.next();
        let mut depth = 1;
        let mut out = Vec::new();
        while let Some(l) = self.next() {
            match l.key.as_str() {
                "{" => depth += 1,
                "}" => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(out);
                    }
                }
                _ => {}
            }
            out.push(l);
        }
        diag.error(
            opener.number,
            format!("блок `{}` не закрыт скобкой `}}`", opener.key),
        );
        None
    }
}

/// Обязательная строка файла: если её нет, пишет ошибку.
fn required<T>(v: Option<T>, key: &str, diag: &mut Diag) -> Option<T> {
    if v.is_none() {
        diag.error(0, format!("в файле нет обязательной строки `{key}`"));
    }
    v
}

fn extra_field(l: &Line) -> ExtraField {
    ExtraField {
        line: l.number,
        key: l.key.clone(),
        value: l.value.clone(),
    }
}

fn faction_line(f: &mut StratFaction, l: &Line, p: &mut Parser, diag: &mut Diag) {
    match l.key.as_str() {
        "ai_label" => f.ai_label = Some(l.value.clone()),
        "denari" => f.denari = parse_num(l, &l.value, "сумма денег", diag).unwrap_or(0),
        "denari_kings_purse" => {
            f.kings_purse = parse_num(l, &l.value, "кошелёк короля", diag).unwrap_or(0)
        }
        "settlement" => {
            if let Some(block) = p.block(l, diag)
                && let Some(s) = settlement(l, &block, diag)
            {
                f.settlements.push(s);
            }
        }
        "character" => {
            if let Some(c) = character(l, diag) {
                f.characters.push(c);
            }
        }
        // `army` только объявляет, что дальше идут отряды последнего персонажа.
        "army" => {}
        "unit" => match f.characters.last_mut() {
            Some(c) => {
                if let Some(u) = unit(l, diag) {
                    c.army.push(u);
                }
            }
            None => diag.error(
                l.number,
                "строка `unit` должна идти после персонажа (`character`) и `army`",
            ),
        },
        _ => f.extra.push(extra_field(l)),
    }
}

fn settlement(opener: &Line, block: &[&Line], diag: &mut Diag) -> Option<StratSettlement> {
    let mut level = None;
    let mut region = None;
    let mut year_founded = Some(0);
    let mut population = None;
    let mut plan_set = None;
    let mut faction_creator = None;
    let mut buildings = Vec::new();
    let mut depth = 0;
    for l in block {
        match (depth, l.key.as_str()) {
            (_, "{") => depth += 1,
            (_, "}") => depth -= 1,
            (0, "level") => level = Some(l.value.clone()),
            (0, "region") => region = Some(l.value.clone()),
            (0, "year_founded") => year_founded = parse_num(l, &l.value, "год основания", diag),
            (0, "population") => population = parse_num(l, &l.value, "население", diag),
            (0, "plan_set") => plan_set = Some(l.value.clone()),
            (0, "faction_creator") => faction_creator = Some(l.value.clone()),
            (0, "building") => {}
            (1, "type") => buildings.push(l.value.clone()),
            _ => {}
        }
    }
    for (missing, key) in [
        (level.is_none(), "level"),
        (region.is_none(), "region"),
        (population.is_none(), "population"),
    ] {
        if missing {
            diag.error(
                opener.number,
                format!("в поселении нет обязательной строки `{key}`"),
            );
        }
    }
    Some(StratSettlement {
        level: level?,
        region: region?,
        year_founded: year_founded?,
        population: population?,
        plan_set,
        faction_creator,
        buildings,
        line: opener.number,
    })
}

fn character(l: &Line, diag: &mut Diag) -> Option<StratCharacter> {
    let parts = split_list(&l.value);
    if parts.len() < 2 {
        diag.error(
            l.number,
            "персонаж записывается как `Имя, тип, …, x 10, y 20`",
        );
        return None;
    }
    let mut flags = Vec::new();
    let mut age = None;
    let mut x = None;
    let mut y = None;
    for part in &parts[2..] {
        let mut words = part.split_whitespace();
        match (words.next(), words.next()) {
            (Some("age"), Some(v)) => age = parse_num(l, v, "возраст", diag),
            (Some("x"), Some(v)) => x = parse_num(l, v, "координата x", diag),
            (Some("y"), Some(v)) => y = parse_num(l, v, "координата y", diag),
            _ => flags.push(part.to_string()),
        }
    }
    if x.is_none() || y.is_none() {
        diag.error(
            l.number,
            format!(
                "у персонажа `{}` должны быть координаты `x …, y …`",
                parts[0]
            ),
        );
    }
    Some(StratCharacter {
        name: parts[0].to_string(),
        kind: parts[1].to_string(),
        flags,
        age,
        x: x?,
        y: y?,
        army: Vec::new(),
        line: l.number,
    })
}

/// `Shield Wall Spearmen   exp 1 armour 0 weapon_lvl 0`. Имя юнита может
/// состоять из нескольких слов, поэтому оно — всё до первого `exp`/`armour`/`weapon_lvl`.
fn unit(l: &Line, diag: &mut Diag) -> Option<StratUnit> {
    const KEYS: [&str; 3] = ["exp", "armour", "weapon_lvl"];
    let words: Vec<&str> = l.value.split_whitespace().collect();
    let split = words
        .iter()
        .position(|w| KEYS.contains(w))
        .unwrap_or(words.len());
    if split == 0 {
        diag.error(l.number, "после `unit` должно идти имя юнита");
        return None;
    }
    let name = words[..split].join(" ");
    let mut values = [0u32; 3];
    let rest = &words[split..];
    if !rest.len().is_multiple_of(2) {
        diag.error(
            l.number,
            "после имени юнита идут пары `exp N armour N weapon_lvl N`",
        );
        return None;
    }
    let mut ok = true;
    for pair in rest.chunks(2) {
        match KEYS.iter().position(|k| *k == pair[0]) {
            Some(i) => match parse_num(l, pair[1], pair[0], diag) {
                Some(v) => values[i] = v,
                None => ok = false,
            },
            None => {
                diag.error(
                    l.number,
                    format!(
                        "у отряда неизвестный параметр `{}`; допустимо: exp, armour, weapon_lvl",
                        pair[0]
                    ),
                );
                ok = false;
            }
        }
    }
    ok.then_some(StratUnit {
        name,
        exp: values[0],
        armour: values[1],
        weapon_lvl: values[2],
        line: l.number,
    })
}

/// `1080 summer` → [`Date`].
fn date(l: &Line, diag: &mut Diag) -> Option<Date> {
    let mut words = l.value.split_whitespace();
    let (Some(year), Some(season), None) = (words.next(), words.next(), words.next()) else {
        diag.error(
            l.number,
            format!("`{}` записывается как `1080 summer`", l.key),
        );
        return None;
    };
    let year = parse_num(l, year, "год", diag)?;
    let season = match season {
        "summer" => Season::Summer,
        "winter" => Season::Winter,
        other => {
            diag.error(
                l.number,
                format!("время года должно быть `summer` или `winter`, а написано `{other}`"),
            );
            return None;
        }
    };
    Some(Date { year, season })
}

/// `2.00` → 200, `0.5` → 50. Без float, чтобы даты считались одинаково везде.
fn hundredths(l: &Line, diag: &mut Diag) -> Option<u32> {
    let v = l.value.trim();
    let (int, frac) = v.split_once('.').unwrap_or((v, ""));
    let bad = || format!("`timescale` должен быть числом вида `2.00`, а написано `{v}`");
    if frac.len() > 2 || !frac.chars().all(|c| c.is_ascii_digit()) {
        diag.error(l.number, bad());
        return None;
    }
    let Ok(int) = int.parse::<u32>() else {
        diag.error(l.number, bad());
        return None;
    };
    let frac = format!("{frac:0<2}").parse::<u32>().unwrap_or(0);
    let total = int * 100 + frac;
    if total == 0 {
        diag.error(l.number, "`timescale` должен быть больше нуля");
        return None;
    }
    Some(total)
}
