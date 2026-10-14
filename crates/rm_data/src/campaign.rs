//! Данные кампании: всё, что нужно правилам кампании (`rm_campaign`),
//! одной структурой [`CampaignData`].
//!
//! Здесь же два новых файла с префиксом `descr_rm_` (их не было в Medieval II,
//! см. `docs/adr/modding-contract.md`):
//!
//! * `descr_rm_campaign.txt` — числа экономики и движения;
//! * `world/maps/base/descr_rm_regions.txt` — где на карте стоит поселение
//!   региона и с какими регионами он граничит. В оригинале это вычислялось
//!   из картинки `map_regions.tga`; пока её разбора нет, соседство задаётся
//!   текстом.

use std::collections::BTreeMap;
use std::path::Path;

use crate::error::{DataError, DataErrors, Diag};
use crate::lexer::{Line, split_list, tokenize};
use crate::record::{ExtraField, Record, group, parse_num};
use crate::region::{REGIONS_FILE, Region, load_regions};
use crate::resource::{RESOURCES_FILE, Resource, load_resources};
use crate::strat::{STRAT_FILE, Strat, load_strat};
use crate::{FACTIONS_FILE, GameData, UNITS_FILE};

/// Путь к файлу правил кампании внутри `data/`.
pub const CAMPAIGN_RULES_FILE: &str = "descr_rm_campaign.txt";
/// Путь к файлу соседства регионов внутри `data/`.
pub const REGION_LINKS_FILE: &str = "world/maps/base/descr_rm_regions.txt";

/// Уровень поселения и его предельное население.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettlementLevel {
    pub name: String,
    pub population_cap: u32,
}

/// Числа правил кампании из `descr_rm_campaign.txt`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CampaignRules {
    /// Налог: денариев за ход с каждых 100 жителей.
    pub tax_per_100_population: u32,
    /// Доход с ферм за ход на каждый уровень плодородия региона.
    pub farm_income_per_level: u32,
    /// Базовый прирост населения за ход, в тысячных (5 = 0,5%).
    pub growth_base_permille: u32,
    /// Добавка к приросту за каждый уровень плодородия, в тысячных.
    pub growth_per_farm_level_permille: u32,
    /// Сколько регионов за ход проходит армия.
    pub army_moves_per_turn: u32,
    /// Сколько регионов за ход проходит армия только из конницы.
    pub cavalry_army_moves_per_turn: u32,
    /// Сколько отрядов поселение обучает одновременно.
    pub recruit_slots_per_turn: u32,
    /// Разброс силы в автобое, ± проценты.
    pub autoresolve_spread_percent: u32,
    /// Потери победителя: сколько процентов от соотношения сил он теряет.
    pub autoresolve_winner_loss_percent: u32,
    /// Уровни поселений от меньшего к большему.
    pub settlement_levels: Vec<SettlementLevel>,
    pub extra: Vec<ExtraField>,
}

impl CampaignRules {
    pub fn level(&self, name: &str) -> Option<usize> {
        self.settlement_levels.iter().position(|l| l.name == name)
    }
}

/// Положение и соседи региона из `descr_rm_regions.txt`.
#[derive(Debug, Clone, PartialEq)]
pub struct RegionLink {
    pub name: String,
    /// Клетка поселения на карте кампании.
    pub x: i32,
    pub y: i32,
    /// Соседние регионы, куда армия может перейти за один шаг.
    pub neighbours: Vec<String>,
    pub line: usize,
    pub extra: Vec<ExtraField>,
}

/// Всё, что нужно для запуска кампании.
#[derive(Debug, Clone)]
pub struct CampaignData {
    pub game: GameData,
    pub regions: Vec<Region>,
    pub resources: Vec<Resource>,
    pub region_links: Vec<RegionLink>,
    pub rules: CampaignRules,
    pub strat: Strat,
}

impl CampaignData {
    /// Загружает все файлы кампании из папки `dir` и проверяет связи между
    /// ними. Возвращает все найденные ошибки разом.
    pub fn load_dir(dir: &Path) -> Result<Self, DataErrors> {
        let mut errors = Vec::new();
        let game = keep(GameData::load_dir(dir), &mut errors);
        let regions = keep(load_regions(&dir.join(REGIONS_FILE)), &mut errors);
        let resources = keep(load_resources(&dir.join(RESOURCES_FILE)), &mut errors);
        let region_links = keep(load_region_links(&dir.join(REGION_LINKS_FILE)), &mut errors);
        let rules = keep(
            load_campaign_rules(&dir.join(CAMPAIGN_RULES_FILE)),
            &mut errors,
        );
        let strat = keep(load_strat(&dir.join(STRAT_FILE)), &mut errors);
        if let (
            Some(game),
            Some(regions),
            Some(resources),
            Some(region_links),
            Some(rules),
            Some(strat),
        ) = (game, regions, resources, region_links, rules, strat)
        {
            let data = CampaignData {
                game,
                regions,
                resources,
                region_links,
                rules,
                strat,
            };
            errors.extend(data.check_references(dir));
            if errors.is_empty() {
                return Ok(data);
            }
        }
        Err(DataErrors(errors))
    }

    /// Проверяет ссылки между файлами кампании. `dir` — папка данных, для сообщений.
    pub fn check_references(&self, dir: &Path) -> Vec<DataError> {
        let mut errors = Vec::new();
        let regions_file = dir.join(REGIONS_FILE);
        let links_file = dir.join(REGION_LINKS_FILE);
        let strat_file = dir.join(STRAT_FILE);
        let mut err = |file: &Path, line: usize, msg: String| {
            errors.push(DataError::new(file, line, msg));
        };

        for r in &self.regions {
            for res in &r.resources {
                if self.resource(res).is_none() {
                    err(
                        &regions_file,
                        r.line,
                        format!(
                            "регион `{}`: ресурс `{res}` не объявлен в {RESOURCES_FILE}",
                            r.name
                        ),
                    );
                }
            }
            if self.region_link(&r.name).is_none() {
                err(
                    &regions_file,
                    r.line,
                    format!(
                        "регион `{}` не описан в {REGION_LINKS_FILE} (нужны положение и соседи)",
                        r.name
                    ),
                );
            }
        }

        for link in &self.region_links {
            if self.region(&link.name).is_none() {
                err(
                    &links_file,
                    link.line,
                    format!("регион `{}` не объявлен в {REGIONS_FILE}", link.name),
                );
            }
            for n in &link.neighbours {
                if n == &link.name {
                    err(
                        &links_file,
                        link.line,
                        format!("регион `{}` указан соседом самого себя", link.name),
                    );
                    continue;
                }
                match self.region_link(n) {
                    None => err(
                        &links_file,
                        link.line,
                        format!("регион `{}`: сосед `{n}` не описан в этом файле", link.name),
                    ),
                    Some(other) if !other.neighbours.contains(&link.name) => err(
                        &links_file,
                        link.line,
                        format!(
                            "регион `{}` граничит с `{n}`, но у `{n}` (строка {}) нет `{}` в соседях; соседство должно быть взаимным",
                            link.name, other.line, link.name
                        ),
                    ),
                    Some(_) => {}
                }
            }
        }

        let mut owner_of: BTreeMap<&str, (&str, usize)> = BTreeMap::new();
        for f in &self.strat.factions {
            if self.game.faction(&f.name).is_none() {
                err(
                    &strat_file,
                    f.line,
                    format!("фракция `{}` не объявлена в {FACTIONS_FILE}", f.name),
                );
            }
            for s in &f.settlements {
                if self.region(&s.region).is_none() {
                    err(
                        &strat_file,
                        s.line,
                        format!(
                            "поселение фракции `{}`: регион `{}` не объявлен в {REGIONS_FILE}",
                            f.name, s.region
                        ),
                    );
                }
                if self.rules.level(&s.level).is_none() {
                    err(
                        &strat_file,
                        s.line,
                        format!(
                            "неизвестный уровень поселения `{}`; уровни задаются в {CAMPAIGN_RULES_FILE}",
                            s.level
                        ),
                    );
                }
                if let Some((other, line)) = owner_of.insert(&s.region, (&f.name, s.line)) {
                    err(
                        &strat_file,
                        s.line,
                        format!(
                            "в регионе `{}` уже стоит поселение фракции `{other}` (строка {line})",
                            s.region
                        ),
                    );
                }
            }
            for c in &f.characters {
                for u in &c.army {
                    match self.game.unit(&u.name) {
                        None => err(
                            &strat_file,
                            u.line,
                            format!("юнит `{}` не объявлен в {UNITS_FILE}", u.name),
                        ),
                        Some(unit) if unit.soldier.count == 0 => err(
                            &strat_file,
                            u.line,
                            format!("у юнита `{}` ноль солдат", u.name),
                        ),
                        Some(_) => {}
                    }
                }
            }
        }
        errors
    }

    pub fn region(&self, name: &str) -> Option<&Region> {
        self.regions.iter().find(|r| r.name == name)
    }

    pub fn region_link(&self, name: &str) -> Option<&RegionLink> {
        self.region_links.iter().find(|r| r.name == name)
    }

    pub fn resource(&self, name: &str) -> Option<&Resource> {
        self.resources.iter().find(|r| r.name == name)
    }
}

/// Значение из результата; ошибки переезжают в общий список.
fn keep<T>(r: Result<T, DataErrors>, errors: &mut Vec<DataError>) -> Option<T> {
    r.map_err(|e| errors.extend(e.0)).ok()
}

/// Разбирает текст `descr_rm_regions.txt`.
pub fn parse_region_links(source: &str, file: &Path) -> Result<Vec<RegionLink>, DataErrors> {
    let mut diag = Diag::new(file);
    let records = group(tokenize(source), "region", &mut diag);
    let mut out: Vec<RegionLink> = Vec::new();
    for record in records {
        if let Some(link) = parse_region_link(record, &mut diag) {
            if let Some(prev) = out.iter().find(|x| x.name == link.name) {
                diag.error(
                    link.line,
                    format!("регион `{}` уже описан на строке {}", link.name, prev.line),
                );
                continue;
            }
            out.push(link);
        }
    }
    diag.finish(out)
}

/// Читает и разбирает файл соседства регионов с диска.
pub fn load_region_links(path: &Path) -> Result<Vec<RegionLink>, DataErrors> {
    let source = crate::read_file(path)?;
    parse_region_links(&source, path)
}

fn parse_region_link(mut r: Record, diag: &mut Diag) -> Option<RegionLink> {
    let position = r.require("position", diag).and_then(|l| {
        let parts = split_list(&l.value);
        if parts.len() != 2 {
            diag.error(l.number, "положение записывается как `position x, y`");
            return None;
        }
        let x = parse_num::<i32>(&l, parts[0], "координата x", diag);
        let y = parse_num::<i32>(&l, parts[1], "координата y", diag);
        Some((x?, y?))
    });
    let neighbours = r
        .require("neighbours", diag)
        .map(|l| {
            split_list(&l.value)
                .into_iter()
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let name = r.name.clone();
    let line = r.start_line;
    let extra = r.into_extra();
    let (x, y) = position?;
    Some(RegionLink {
        name,
        x,
        y,
        neighbours,
        line,
        extra,
    })
}

/// Разбирает текст `descr_rm_campaign.txt`.
pub fn parse_campaign_rules(source: &str, file: &Path) -> Result<CampaignRules, DataErrors> {
    let mut diag = Diag::new(file);
    let mut single: BTreeMap<String, Line> = BTreeMap::new();
    let mut levels: Vec<SettlementLevel> = Vec::new();
    let mut extra = Vec::new();
    const KEYS: [&str; 9] = [
        "tax_per_100_population",
        "farm_income_per_level",
        "growth_base_permille",
        "growth_per_farm_level_permille",
        "army_moves_per_turn",
        "cavalry_army_moves_per_turn",
        "recruit_slots_per_turn",
        "autoresolve_spread_percent",
        "autoresolve_winner_loss_percent",
    ];
    for l in tokenize(source) {
        if l.key == "settlement_level" {
            let parts = split_list(&l.value);
            if parts.len() != 2 {
                diag.error(
                    l.number,
                    "уровень записывается как `settlement_level <имя>, <предел населения>`",
                );
                continue;
            }
            if levels.iter().any(|x| x.name == parts[0]) {
                diag.error(l.number, format!("уровень `{}` уже объявлен", parts[0]));
                continue;
            }
            if let Some(cap) = parse_num::<u32>(&l, parts[1], "предел населения", &mut diag)
            {
                levels.push(SettlementLevel {
                    name: parts[0].to_string(),
                    population_cap: cap,
                });
            }
        } else if KEYS.contains(&l.key.as_str()) {
            if let Some(prev) = single.get(&l.key) {
                diag.error(
                    l.number,
                    format!("`{}` уже задан на строке {}", l.key, prev.number),
                );
            } else {
                single.insert(l.key.clone(), l);
            }
        } else {
            extra.push(ExtraField {
                line: l.number,
                key: l.key,
                value: l.value,
            });
        }
    }
    let mut num = |key: &str| -> Option<u32> {
        match single.get(key) {
            Some(l) => parse_num(l, &l.value, "значение", &mut diag),
            None => {
                diag.error(0, format!("в файле нет обязательной строки `{key}`"));
                None
            }
        }
    };
    let values: Vec<Option<u32>> = KEYS.iter().map(|k| num(k)).collect();
    if levels.is_empty() {
        diag.error(0, "нужен хотя бы один `settlement_level`");
    }
    if values.iter().any(Option::is_none) {
        return Err(DataErrors(diag.errors));
    }
    let v: Vec<u32> = values.into_iter().flatten().collect();
    diag.finish(CampaignRules {
        tax_per_100_population: v[0],
        farm_income_per_level: v[1],
        growth_base_permille: v[2],
        growth_per_farm_level_permille: v[3],
        army_moves_per_turn: v[4],
        cavalry_army_moves_per_turn: v[5],
        recruit_slots_per_turn: v[6],
        autoresolve_spread_percent: v[7],
        autoresolve_winner_loss_percent: v[8],
        settlement_levels: levels,
        extra,
    })
}

/// Читает и разбирает файл правил кампании с диска.
pub fn load_campaign_rules(path: &Path) -> Result<CampaignRules, DataErrors> {
    let source = crate::read_file(path)?;
    parse_campaign_rules(&source, path)
}
