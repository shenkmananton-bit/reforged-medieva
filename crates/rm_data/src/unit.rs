//! Юниты: формат `export_descr_unit.txt`.
//!
//! Каждый юнит начинается со строки `type <имя>`. Обязательные поля
//! разбираются в типизированные структуры, необязательные — в `Option`,
//! неизвестные сохраняются в [`Unit::extra`].

use std::path::Path;

use crate::error::{DataErrors, Diag};
use crate::lexer::{split_list, tokenize};
use crate::record::{
    ExtraField, Record, TransposeInner, Values, group, names, parse_num, pick, single_word, text,
};

/// Имя файла юнитов в папке `data/`.
pub const UNITS_FILE: &str = "export_descr_unit.txt";

/// Род войск (`category`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnitCategory {
    Infantry,
    Cavalry,
    Siege,
    Handler,
    Ship,
    NonCombatant,
}

impl UnitCategory {
    const ALL: &[(&str, UnitCategory)] = &[
        ("infantry", Self::Infantry),
        ("cavalry", Self::Cavalry),
        ("siege", Self::Siege),
        ("handler", Self::Handler),
        ("ship", Self::Ship),
        ("non_combatant", Self::NonCombatant),
    ];
}

/// Класс юнита внутри рода войск (`class`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnitClass {
    Light,
    Heavy,
    Missile,
    Spearmen,
    Skirmish,
}

impl UnitClass {
    const ALL: &[(&str, UnitClass)] = &[
        ("light", Self::Light),
        ("heavy", Self::Heavy),
        ("missile", Self::Missile),
        ("spearmen", Self::Spearmen),
        ("skirmish", Self::Skirmish),
    ];
}

/// `soldier <модель>, <число>, <доп. фигуры>, <масса>`
#[derive(Debug, Clone, PartialEq)]
pub struct Soldier {
    /// Имя модели солдата.
    pub model: String,
    /// Сколько солдат в отряде при обычном размере отряда.
    pub count: u32,
    /// Дополнительные фигуры (животные у погонщиков, орудия у артиллерии).
    pub extras: u32,
    /// Масса солдата для расталкивания (soft collision). По умолчанию 1.0.
    pub mass: f32,
}

/// `formation <плотно бок>, <плотно глубина>, <рассып. бок>, <рассып. глубина>, <шеренги>, <строй>...`
#[derive(Debug, Clone, PartialEq)]
pub struct Formation {
    pub tight_side: f32,
    pub tight_back: f32,
    pub loose_side: f32,
    pub loose_back: f32,
    /// Число шеренг по умолчанию.
    pub ranks: u32,
    /// Доступные построения: `square`, `wedge`, `horde`, …
    pub formations: Vec<String>,
}

/// `stat_health <здоровье солдата>, <здоровье ездового животного>`
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Health {
    pub soldier: u32,
    pub mount: u32,
}

/// Оружие: строки `stat_pri` (основное) и `stat_sec` (второе).
///
/// `атака, бонус натиска, снаряд, дальность, боезапас, тип оружия,
///  технология, тип урона, звук[, задержка между ударами[, коэффициент скелета]]`
#[derive(Debug, Clone, PartialEq)]
pub struct Weapon {
    pub attack: u32,
    pub charge_bonus: u32,
    /// Тип снаряда; `no` в файле превращается в `None`.
    pub missile: Option<String>,
    pub range: u32,
    pub ammo: u32,
    /// `melee`, `missile`, `thrown`, `siege_missile`, `no`.
    pub weapon_type: String,
    pub tech_type: String,
    /// `piercing`, `blunt`, `slashing`, `fire`.
    pub damage_type: String,
    pub sound_type: String,
    /// Минимальная задержка между ударами, в десятых долях секунды. По умолчанию 25.
    pub min_delay: u32,
    /// Поправка анимации на размер скелета. По умолчанию 1.0.
    pub skeleton_factor: f32,
}

/// `stat_pri_armour <броня>, <защита>, <щит>, <звук>`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Armour {
    pub armour: u32,
    pub defence_skill: u32,
    pub shield: u32,
    pub sound: String,
}

/// `stat_mental <мораль>, <дисциплина>, <выучка>[, флаги...]`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mental {
    pub morale: i32,
    /// `low`, `normal`, `disciplined`, `impetuous`, `berserker`.
    pub discipline: String,
    /// `untrained`, `trained`, `highly_trained`.
    pub training: String,
    /// Дополнительные флаги, например `lock_morale`.
    pub flags: Vec<String>,
}

/// `stat_cost <ходы>, <цена>, <содержание>, <апгрейд оружия>, <апгрейд брони>,
///            <цена в своей битве>, <лимит в своей битве>, <штраф за превышение>`
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cost {
    pub recruit_turns: u32,
    pub price: u32,
    pub upkeep: u32,
    pub weapon_upgrade: u32,
    pub armour_upgrade: u32,
    pub custom_battle_price: u32,
    pub custom_battle_limit: u32,
    pub custom_battle_penalty: u32,
}

/// Один юнит из `export_descr_unit.txt`.
#[derive(Debug, Clone, PartialEq)]
pub struct Unit {
    /// Внутреннее имя (`type`). По нему юнит ищут остальные файлы.
    pub name: String,
    /// Ключ для текстов интерфейса (`dictionary`).
    pub dictionary: String,
    pub category: UnitCategory,
    pub class: UnitClass,
    pub voice_type: Option<String>,
    /// Знамя отряда (`banner faction`).
    pub banner_faction: Option<String>,
    /// Знамя крестового похода/джихада (`banner holy`).
    pub banner_holy: Option<String>,
    pub soldier: Soldier,
    /// Офицеры (`officer`, может повторяться).
    pub officers: Vec<String>,
    /// Ездовое животное (`mount`), если есть.
    pub mount: Option<String>,
    /// Атрибуты (`attributes`): `can_withdraw`, `hide_forest`, …
    pub attributes: Vec<String>,
    pub formation: Option<Formation>,
    pub health: Health,
    pub primary_weapon: Weapon,
    pub secondary_weapon: Option<Weapon>,
    pub armour: Armour,
    pub mental: Mental,
    /// Дистанция начала натиска (`stat_charge_dist`).
    pub charge_distance: Option<u32>,
    pub cost: Cost,
    /// Фракции, которые могут нанимать юнит (`ownership`).
    pub ownership: Vec<String>,
    /// Строка, на которой начинается юнит.
    pub line: usize,
    /// Поля, которые парсер пока не знает.
    pub extra: Vec<ExtraField>,
}

/// Разбирает текст `export_descr_unit.txt`.
///
/// `file` нужен только для сообщений об ошибках. При ошибках возвращает
/// их все сразу, а не только первую.
pub fn parse_units(source: &str, file: &Path) -> Result<Vec<Unit>, DataErrors> {
    let mut diag = Diag::new(file);
    let records = group(tokenize(source), "type", &mut diag);
    let mut units: Vec<Unit> = Vec::new();
    for record in records {
        if let Some(unit) = parse_unit(record, &mut diag) {
            if let Some(prev) = units.iter().find(|u| u.name == unit.name) {
                diag.error(
                    unit.line,
                    format!("юнит `{}` уже объявлен на строке {}", unit.name, prev.line),
                );
                continue;
            }
            units.push(unit);
        }
    }
    diag.finish(units)
}

/// Читает и разбирает файл юнитов с диска.
pub fn load_units(path: &Path) -> Result<Vec<Unit>, DataErrors> {
    let source = crate::read_file(path)?;
    parse_units(&source, path)
}

fn parse_unit(mut r: Record, diag: &mut Diag) -> Option<Unit> {
    // Каждое поле разбираем независимо, чтобы собрать все ошибки записи сразу.
    let dictionary = r
        .require("dictionary", diag)
        .and_then(|l| single_word(&l, diag));
    let category = r.require("category", diag).and_then(|l| {
        pick(&l.value, UnitCategory::ALL).or_else(|| {
            diag.error(
                l.number,
                format!(
                    "неизвестная категория `{}`; допустимо: {}",
                    l.value,
                    names(UnitCategory::ALL)
                ),
            );
            None
        })
    });
    let class = r.require("class", diag).and_then(|l| {
        pick(&l.value, UnitClass::ALL).or_else(|| {
            diag.error(
                l.number,
                format!(
                    "неизвестный класс `{}`; допустимо: {}",
                    l.value,
                    names(UnitClass::ALL)
                ),
            );
            None
        })
    });
    let voice_type = r
        .take("voice_type", diag)
        .and_then(|l| single_word(&l, diag));
    let banner_faction = r
        .take("banner faction", diag)
        .and_then(|l| single_word(&l, diag));
    let banner_holy = r
        .take("banner holy", diag)
        .and_then(|l| single_word(&l, diag));
    let soldier = r.require("soldier", diag).and_then(|l| {
        let v = Values::new(&l, 3, "модель, число, доп. фигуры[, масса]", diag)?;
        let count = v.num(1, "число солдат", diag);
        let extras = v.num(2, "число доп. фигур", diag);
        let mass = if v.len() > 3 {
            v.num(3, "масса", diag)
        } else {
            Some(1.0)
        };
        Some(Soldier {
            model: v.str(0),
            count: count?,
            extras: extras?,
            mass: mass?,
        })
    });
    let officers = r
        .take_all("officer")
        .into_iter()
        .filter_map(|l| single_word(&l, diag))
        .collect();
    let mount = r.take("mount", diag).and_then(|l| text(&l, diag));
    let attributes = r
        .take("attributes", diag)
        .map(|l| {
            split_list(&l.value)
                .into_iter()
                .filter(|s| !s.is_empty())
                .map(String::from)
                .collect()
        })
        .unwrap_or_default();
    let formation = r.take("formation", diag).map(|l| {
        let v = Values::new(
            &l,
            6,
            "плотно бок, плотно глубина, рассып. бок, рассып. глубина, шеренги, строй...",
            diag,
        )?;
        let tight_side = v.num(0, "плотный интервал сбоку", diag);
        let tight_back = v.num(1, "плотный интервал сзади", diag);
        let loose_side = v.num(2, "рассыпной интервал сбоку", diag);
        let loose_back = v.num(3, "рассыпной интервал сзади", diag);
        let ranks = v.num(4, "число шеренг", diag);
        Some(Formation {
            tight_side: tight_side?,
            tight_back: tight_back?,
            loose_side: loose_side?,
            loose_back: loose_back?,
            ranks: ranks?,
            formations: v.rest(5),
        })
    });
    let health = r.require("stat_health", diag).and_then(|l| {
        let v = Values::new(&l, 2, "здоровье солдата, здоровье животного", diag)?;
        let soldier = v.num(0, "здоровье солдата", diag);
        let mount = v.num(1, "здоровье животного", diag);
        Some(Health {
            soldier: soldier?,
            mount: mount?,
        })
    });
    let primary_weapon = r.require("stat_pri", diag).and_then(|l| weapon(&l, diag));
    let secondary_weapon = r.take("stat_sec", diag).map(|l| weapon(&l, diag));
    let armour = r.require("stat_pri_armour", diag).and_then(|l| {
        let v = Values::new(&l, 4, "броня, защита, щит, звук", diag)?;
        let armour = v.num(0, "броня", diag);
        let defence = v.num(1, "навык защиты", diag);
        let shield = v.num(2, "щит", diag);
        Some(Armour {
            armour: armour?,
            defence_skill: defence?,
            shield: shield?,
            sound: v.str(3),
        })
    });
    let mental = r.require("stat_mental", diag).and_then(|l| {
        let v = Values::new(&l, 3, "мораль, дисциплина, выучка", diag)?;
        let morale = v.num(0, "мораль", diag);
        Some(Mental {
            morale: morale?,
            discipline: v.str(1),
            training: v.str(2),
            flags: v.rest(3),
        })
    });
    let charge_distance = r
        .take("stat_charge_dist", diag)
        .map(|l| parse_num(&l, &l.value, "дистанция натиска", diag));
    let cost = r.require("stat_cost", diag).and_then(|l| {
        let v = Values::new(
            &l,
            8,
            "ходы, цена, содержание, апгрейд оружия, апгрейд брони, цена/лимит/штраф своей битвы",
            diag,
        )?;
        let what = [
            "ходы найма",
            "цена",
            "содержание",
            "цена апгрейда оружия",
            "цена апгрейда брони",
            "цена в своей битве",
            "лимит в своей битве",
            "штраф за превышение лимита",
        ];
        let n: Vec<Option<u32>> = what
            .iter()
            .enumerate()
            .map(|(i, w)| v.num(i, w, diag))
            .collect();
        Some(Cost {
            recruit_turns: n[0]?,
            price: n[1]?,
            upkeep: n[2]?,
            weapon_upgrade: n[3]?,
            armour_upgrade: n[4]?,
            custom_battle_price: n[5]?,
            custom_battle_limit: n[6]?,
            custom_battle_penalty: n[7]?,
        })
    });
    let ownership = r.require("ownership", diag).and_then(|l| {
        let list: Vec<String> = split_list(&l.value).into_iter().map(String::from).collect();
        if list.iter().any(String::is_empty) {
            diag.error(
                l.number,
                "в `ownership` пустое имя фракции (лишняя запятая?)",
            );
            return None;
        }
        Some(list)
    });

    let name = r.name.clone();
    let line = r.start_line;
    let extra = r.into_extra();
    Some(Unit {
        name,
        dictionary: dictionary?,
        category: category?,
        class: class?,
        voice_type,
        banner_faction,
        banner_holy,
        soldier: soldier?,
        officers,
        mount,
        attributes,
        formation: formation.transpose_inner()?,
        health: health?,
        primary_weapon: primary_weapon?,
        secondary_weapon: secondary_weapon.transpose_inner()?,
        armour: armour?,
        mental: mental?,
        charge_distance: charge_distance.transpose_inner()?,
        cost: cost?,
        ownership: ownership?,
        line,
        extra,
    })
}

fn weapon(l: &crate::lexer::Line, diag: &mut Diag) -> Option<Weapon> {
    let v = Values::new(
        l,
        9,
        "атака, натиск, снаряд, дальность, боезапас, тип, технология, урон, звук",
        diag,
    )?;
    let attack = v.num(0, "атака", diag);
    let charge = v.num(1, "бонус натиска", diag);
    let range = v.num(3, "дальность", diag);
    let ammo = v.num(4, "боезапас", diag);
    let min_delay = if v.len() > 9 {
        v.num(9, "задержка между ударами", diag)
    } else {
        Some(25)
    };
    let skeleton = if v.len() > 10 {
        v.num(10, "коэффициент скелета", diag)
    } else {
        Some(1.0)
    };
    let missile = v.str(2);
    Some(Weapon {
        attack: attack?,
        charge_bonus: charge?,
        missile: if missile == "no" { None } else { Some(missile) },
        range: range?,
        ammo: ammo?,
        weapon_type: v.str(5),
        tech_type: v.str(6),
        damage_type: v.str(7),
        sound_type: v.str(8),
        min_delay: min_delay?,
        skeleton_factor: skeleton?,
    })
}
