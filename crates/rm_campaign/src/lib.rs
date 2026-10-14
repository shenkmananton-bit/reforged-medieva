//! # rm_campaign — правила кампании
//!
//! Пошаговая кампания в духе Medieval II:
//!
//! * фракции ходят по очереди, в порядке из `descr_strat.txt`;
//! * в начале своего хода фракция обучает отряды, её поселения растут,
//!   она получает налоги, доход с ферм и торговли, кошелёк короля и платит
//!   за содержание армий ([`economy`]);
//! * армии переходят между соседними регионами; встреча с чужой армией —
//!   автобой, вход в чужой регион без защитников — захват поселения ([`army`]).
//!
//! Все числа берутся из текстовых файлов через `rm_data` (см.
//! `docs/modding/campaign.md`), поселения и армии живут в ECS из `rm_core`,
//! случайность — только из детерминированного ГСЧ. Мир меняется только
//! через [`Campaign::apply`] и [`Campaign::end_turn`], поэтому кампанию
//! можно повторить по списку команд (реплей) и сверить [`Campaign::state_hash`].
//!
//! Подробно по-русски: `docs/campaign/`.

pub mod army;
pub mod autoplay;
pub mod command;
pub mod components;
pub mod economy;
pub mod ids;
pub mod map;

use std::hash::{Hash, Hasher};

use rm_core::sim::StableHasher;
use rm_core::{Entity, World};
use rm_data::{CampaignData, CampaignRules, Date, Season, UnitCategory};

pub use command::{CampaignEvent, Command, CommandError, IncomeReport};
pub use components::{Army, ArmyUnit, Location, Moves, Owner, Recruit, RecruitQueue, Settlement};
pub use ids::{FactionId, RegionId, UnitTypeId};
pub use map::{CampaignMap, RegionInfo};

/// Имя фракции мятежников, как в оригинале.
pub const REBELS: &str = "slave";

/// Состояние фракции.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FactionState {
    pub id: FactionId,
    pub name: String,
    /// Можно ли за неё играть (`playable` в `descr_strat.txt`).
    pub playable: bool,
    /// Деньги в казне. Могут уйти в минус, как в оригинале.
    pub treasury: i64,
    /// Деньги, которые фракция получает каждый ход сверху.
    pub kings_purse: i64,
    /// Жива ли фракция: есть хоть одно поселение или армия.
    pub alive: bool,
    /// Доходы и расходы в начале последнего хода фракции.
    pub last_income: IncomeReport,
}

/// Тип юнита в кампании: то, что правилам нужно из `export_descr_unit.txt`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct UnitType {
    pub id: UnitTypeId,
    pub name: String,
    /// Полная численность отряда.
    pub soldiers: u32,
    /// Цена найма.
    pub price: i64,
    /// Содержание за ход.
    pub upkeep: i64,
    /// Ходов на обучение.
    pub recruit_turns: u32,
    /// Конница (армия только из конницы ходит дальше).
    pub cavalry: bool,
    /// Сила одного солдата в автобое.
    pub power_per_soldier: u64,
    /// Фракции, которые могут нанимать юнит.
    pub ownership: Vec<FactionId>,
}

/// Снимок поселения для чтения (интерфейс, рендер карты, ИИ).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettlementView {
    pub entity: Entity,
    pub region: RegionId,
    pub owner: FactionId,
    pub level: u8,
    pub population: u32,
    pub queue: Vec<Recruit>,
}

/// Снимок армии для чтения.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArmyView {
    pub entity: Entity,
    pub owner: FactionId,
    pub region: RegionId,
    pub moves: u8,
    pub general: Option<String>,
    pub units: Vec<ArmyUnit>,
}

/// Ошибка запуска кампании.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetupError(pub String);

impl std::fmt::Display for SetupError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "кампанию нельзя запустить: {}", self.0)
    }
}

impl std::error::Error for SetupError {}

/// Кампания целиком.
pub struct Campaign {
    pub(crate) world: World,
    pub(crate) map: CampaignMap,
    pub(crate) rules: CampaignRules,
    pub(crate) units: Vec<UnitType>,
    pub(crate) factions: Vec<FactionState>,
    /// Сущность поселения для каждого региона (если поселение есть).
    pub(crate) settlements: Vec<Option<Entity>>,
    pub(crate) seed: u64,
    /// Номер хода, с нуля. Ход — это круг, в котором сходили все фракции.
    pub(crate) turn: u32,
    /// Чей сейчас ход.
    pub(crate) current: FactionId,
    /// Сколько автобоёв уже было: ключ для ГСЧ, чтобы бои не повторяли друг друга.
    pub(crate) battles: u64,
    pub(crate) start_date: Date,
    pub(crate) end_date: Date,
    pub(crate) timescale_hundredths: u32,
    pub(crate) events: Vec<CampaignEvent>,
}

impl Campaign {
    /// Расставляет кампанию по данным и начинает ход первой фракции.
    pub fn new(data: &CampaignData, seed: u64) -> Result<Self, SetupError> {
        let map = CampaignMap::from_data(data);
        if map.regions().is_empty() {
            return Err(SetupError("на карте нет ни одного региона".into()));
        }
        let strat = &data.strat;
        if strat.factions.is_empty() {
            return Err(SetupError("в descr_strat.txt нет ни одной фракции".into()));
        }
        let mut rules = data.rules.clone();
        // Удача больше 100% дала бы отрицательную силу.
        rules.autoresolve_spread_percent = rules.autoresolve_spread_percent.min(99);

        let factions: Vec<FactionState> = strat
            .factions
            .iter()
            .enumerate()
            .map(|(i, f)| FactionState {
                id: FactionId(i as u16),
                name: f.name.clone(),
                playable: strat.playable.contains(&f.name),
                treasury: f.denari,
                kings_purse: f.kings_purse,
                alive: true,
                last_income: IncomeReport::default(),
            })
            .collect();
        let faction_id = |name: &str| factions.iter().find(|f| f.name == name).map(|f| f.id);

        let units: Vec<UnitType> = data
            .game
            .units
            .iter()
            .enumerate()
            .map(|(i, u)| {
                let per_soldier = u64::from(u.primary_weapon.attack)
                    + u64::from(u.armour.armour)
                    + u64::from(u.armour.defence_skill)
                    + u64::from(u.armour.shield)
                    + u64::from(u.mental.morale.max(0) as u32);
                UnitType {
                    id: UnitTypeId(i as u16),
                    name: u.name.clone(),
                    soldiers: u.soldier.count,
                    price: i64::from(u.cost.price),
                    upkeep: i64::from(u.cost.upkeep),
                    recruit_turns: u.cost.recruit_turns,
                    cavalry: u.category == UnitCategory::Cavalry,
                    power_per_soldier: per_soldier * u64::from(u.health.soldier.max(1)),
                    ownership: u.ownership.iter().filter_map(|o| faction_id(o)).collect(),
                }
            })
            .collect();

        let mut world = World::new();
        let mut settlements = vec![None; map.regions().len()];
        for (fi, f) in strat.factions.iter().enumerate() {
            let owner = Owner(FactionId(fi as u16));
            for s in &f.settlements {
                let (Some(region), Some(level)) = (map.find(&s.region), rules.level(&s.level))
                else {
                    return Err(SetupError(format!(
                        "поселение в регионе `{}` ссылается на неизвестные данные",
                        s.region
                    )));
                };
                let cap = rules.settlement_levels[level].population_cap;
                let e = world.spawn((
                    Settlement {
                        region,
                        level: level as u8,
                        population: s.population.min(cap),
                    },
                    owner,
                    RecruitQueue::default(),
                ));
                settlements[region.index()] = Some(e);
            }
        }
        for (fi, f) in strat.factions.iter().enumerate() {
            let owner = Owner(FactionId(fi as u16));
            for c in &f.characters {
                let army_units: Vec<ArmyUnit> = c
                    .army
                    .iter()
                    .filter_map(|u| {
                        let t = units.iter().find(|t| t.name == u.name)?;
                        Some(ArmyUnit {
                            unit: t.id,
                            soldiers: t.soldiers,
                            exp: u.exp,
                        })
                    })
                    .collect();
                // Персонажи без армии (шпионы, торговцы) — дело будущих агентов.
                if army_units.is_empty() {
                    continue;
                }
                let region = map.nearest(c.x, c.y).expect("карта не пуста");
                world.spawn((
                    Army {
                        general: Some(c.name.clone()),
                        units: army_units,
                    },
                    owner,
                    Location(region),
                    Moves(0),
                ));
            }
        }

        let mut campaign = Self {
            world,
            map,
            rules,
            units,
            factions,
            settlements,
            seed,
            turn: 0,
            current: FactionId(0),
            battles: 0,
            start_date: strat.start_date,
            end_date: strat.end_date,
            timescale_hundredths: strat.timescale_hundredths,
            events: Vec::new(),
        };
        campaign.check_eliminations();
        match campaign.next_alive(0) {
            Some(first) => {
                campaign.current = first;
                campaign.begin_faction_turn();
                Ok(campaign)
            }
            None => Err(SetupError(
                "ни у одной фракции нет поселений или армий".into(),
            )),
        }
    }

    // ------------------------------------------------------------ ход

    /// Выполняет команду фракции, которая сейчас ходит.
    pub fn apply(&mut self, command: Command) -> Result<(), CommandError> {
        if self.is_finished() {
            return Err(CommandError::CampaignOver);
        }
        match command {
            Command::MoveArmy { army, to } => self.move_army(army, to),
            Command::Recruit { region, unit } => self.recruit(region, unit),
        }
    }

    /// Заканчивает ход текущей фракции и начинает ход следующей живой.
    /// Когда круг замыкается, номер хода растёт на единицу.
    pub fn end_turn(&mut self) {
        if self.is_finished() {
            return;
        }
        let from = self.current.index() + 1;
        let next = self.next_alive(from).or_else(|| self.next_alive(0));
        let Some(next) = next else { return };
        if next.index() < from {
            self.turn += 1;
        }
        self.current = next;
        self.begin_faction_turn();
    }

    /// Первая живая фракция начиная с позиции `from` (без перехода по кругу).
    fn next_alive(&self, from: usize) -> Option<FactionId> {
        self.factions
            .iter()
            .skip(from)
            .find(|f| f.alive)
            .map(|f| f.id)
    }

    /// Закончилась ли кампания: прошла дата окончания или осталась одна
    /// фракция, кроме мятежников.
    pub fn is_finished(&self) -> bool {
        let d = self.date();
        let past_end = (d.year, season_rank(d.season))
            > (self.end_date.year, season_rank(self.end_date.season));
        let contenders = self
            .factions
            .iter()
            .filter(|f| f.alive && f.name != REBELS)
            .count();
        past_end || contenders <= 1
    }

    /// Начало хода текущей фракции: найм, рост, доход, восстановление переходов.
    fn begin_faction_turn(&mut self) {
        let f = self.current;
        self.events.push(CampaignEvent::TurnStarted {
            turn: self.turn,
            faction: f,
        });
        self.progress_recruitment(f);
        self.grow_population(f);
        self.collect_income(f);
        self.reset_moves(f);
    }

    /// Убивает фракции без поселений и армий.
    pub(crate) fn check_eliminations(&mut self) {
        let mut has_something = vec![false; self.factions.len()];
        for o in self.world.query::<&Owner>() {
            has_something[o.0.index()] = true;
        }
        for f in &mut self.factions {
            if f.alive && !has_something[f.id.index()] {
                f.alive = false;
                self.events
                    .push(CampaignEvent::FactionDestroyed { faction: f.id });
            }
        }
    }

    // ------------------------------------------------------------ чтение

    /// Номер хода, с нуля.
    pub fn turn(&self) -> u32 {
        self.turn
    }

    /// Дата текущего хода: каждый ход прибавляет `timescale` лет (дробная
    /// часть копится), время года чередуется лето/зима.
    pub fn date(&self) -> Date {
        let years = (u64::from(self.turn) * u64::from(self.timescale_hundredths) / 100) as i32;
        let flip = self.turn % 2 == 1;
        let season = match (self.start_date.season, flip) {
            (s, false) => s,
            (Season::Summer, true) => Season::Winter,
            (Season::Winter, true) => Season::Summer,
        };
        Date {
            year: self.start_date.year + years,
            season,
        }
    }

    /// Чей сейчас ход.
    pub fn current_faction(&self) -> FactionId {
        self.current
    }

    pub fn factions(&self) -> &[FactionState] {
        &self.factions
    }

    pub fn faction(&self, id: FactionId) -> &FactionState {
        &self.factions[id.index()]
    }

    pub fn faction_id(&self, name: &str) -> Option<FactionId> {
        self.factions.iter().find(|f| f.name == name).map(|f| f.id)
    }

    pub fn map(&self) -> &CampaignMap {
        &self.map
    }

    pub fn rules(&self) -> &CampaignRules {
        &self.rules
    }

    pub fn unit_types(&self) -> &[UnitType] {
        &self.units
    }

    pub fn unit_type(&self, id: UnitTypeId) -> &UnitType {
        &self.units[id.index()]
    }

    pub fn unit_type_id(&self, name: &str) -> Option<UnitTypeId> {
        self.units.iter().find(|u| u.name == name).map(|u| u.id)
    }

    /// Мир ECS только на чтение (для систем отображения).
    pub fn world(&self) -> &World {
        &self.world
    }

    /// Все поселения в порядке ECS.
    pub fn settlements(&self) -> Vec<SettlementView> {
        self.world
            .query::<(Entity, &Settlement, &Owner, &RecruitQueue)>()
            .map(|(entity, s, o, q)| SettlementView {
                entity,
                region: s.region,
                owner: o.0,
                level: s.level,
                population: s.population,
                queue: q.0.clone(),
            })
            .collect()
    }

    /// Поселение в регионе.
    pub fn settlement_in(&self, region: RegionId) -> Option<SettlementView> {
        let e = self.settlements[region.index()]?;
        let s = self.world.get::<Settlement>(e)?;
        Some(SettlementView {
            entity: e,
            region: s.region,
            owner: self.world.get::<Owner>(e)?.0,
            level: s.level,
            population: s.population,
            queue: self.world.get::<RecruitQueue>(e)?.0.clone(),
        })
    }

    /// Все армии в порядке ECS.
    pub fn armies(&self) -> Vec<ArmyView> {
        self.world
            .query::<(Entity, &Army, &Owner, &Location, &Moves)>()
            .map(|(entity, a, o, l, m)| ArmyView {
                entity,
                owner: o.0,
                region: l.0,
                moves: m.0,
                general: a.general.clone(),
                units: a.units.clone(),
            })
            .collect()
    }

    pub fn army(&self, entity: Entity) -> Option<ArmyView> {
        self.armies().into_iter().find(|a| a.entity == entity)
    }

    /// Сила армии в автобое (без удачи).
    pub fn army_power(&self, units: &[ArmyUnit]) -> u64 {
        units
            .iter()
            .map(|u| u64::from(u.soldiers) * self.unit_type(u.unit).power_per_soldier)
            .sum()
    }

    /// Содержание армий фракции за ход.
    pub fn upkeep(&self, faction: FactionId) -> i64 {
        self.world
            .query::<(&Army, &Owner)>()
            .filter(|(_, o)| o.0 == faction)
            .flat_map(|(a, _)| a.units.iter())
            .map(|u| self.unit_type(u.unit).upkeep)
            .sum()
    }

    /// Забирает накопленные события.
    pub fn drain_events(&mut self) -> Vec<CampaignEvent> {
        std::mem::take(&mut self.events)
    }

    /// Хеш всего состояния кампании для контрольных точек реплея: одинаковые
    /// данные, seed и команды дают одинаковый хеш на любой машине.
    pub fn state_hash(&self) -> u64 {
        let mut h = StableHasher::new();
        self.turn.hash(&mut h);
        self.current.hash(&mut h);
        self.battles.hash(&mut h);
        self.factions.hash(&mut h);
        h.write_u64(self.world.hash_component::<Settlement>());
        h.write_u64(self.world.hash_component::<RecruitQueue>());
        h.write_u64(self.world.hash_component::<Army>());
        h.write_u64(self.world.hash_component::<Owner>());
        h.write_u64(self.world.hash_component::<Location>());
        h.write_u64(self.world.hash_component::<Moves>());
        h.finish()
    }
}

fn season_rank(s: Season) -> u8 {
    match s {
        Season::Summer => 0,
        Season::Winter => 1,
    }
}

#[cfg(test)]
mod tests;
