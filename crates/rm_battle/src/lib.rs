//! # rm_battle — боевая симуляция
//!
//! Полевая битва в духе Medieval II, собранная из «умных хаков»:
//!
//! * солдат — это цилиндр на плоскости (хаки H-06, H-07), визуальная
//!   модель рисуется отдельно (H-09);
//! * столкновений нет, есть мягкое расталкивание (H-08);
//! * приказы получает отряд, солдаты лишь держат свои места в строю (H-21, B-11);
//! * стрела летит по заранее известной дуге, попадание — по радиусу
//!   и броску кубика (H-10);
//! * всё считается с фиксированным шагом 20 раз в секунду и детерминировано:
//!   один seed и одни команды дают ту же битву до бита.
//!
//! Мир меняется только через [`Command`] в [`Battle::step`]. Рендер читает
//! солдат и снаряды через [`Battle::soldiers`] и [`Battle::projectiles`].
//! Подробно по-русски: `docs/battle/`.

pub mod config;
mod grid;
pub mod math;
pub mod regiment;
mod step;

use std::hash::{Hash, Hasher};

use rm_core::World;
use rm_core::sim::StableHasher;
use rm_core::sim::{Rng, RngStream};
use rm_data::GameData;

pub use config::BattleConfig;
pub use math::Vec2;
pub use regiment::{Order, Regiment, Side, UnitId, UnitState, UnitStats};

/// Солдат: к какому отряду относится и что с ним сейчас.
#[derive(Debug, Clone, Copy, PartialEq, Hash)]
pub struct Soldier {
    pub unit: UnitId,
    pub side: Side,
    /// Место в строю отряда.
    pub slot: u16,
    pub hp: u16,
    /// Шагов до следующего удара или выстрела.
    pub cooldown: u16,
    pub ammo: u16,
    /// Шагов назад был последний удар (для анимации); 255 — давно.
    pub since_strike: u8,
    /// Сражается ли прямо сейчас (для анимации и интерфейса).
    pub fighting: bool,
}

/// Цилиндр солдата на поле.
#[derive(Debug, Clone, Copy, PartialEq, Hash)]
pub struct Body {
    pub pos: Vec2,
    /// Позиция на предыдущем шаге: рендер плавно смешивает `prev` и `pos` (хак H-26).
    pub prev: Vec2,
    /// Куда смотрит солдат (единичный вектор).
    pub facing: Vec2,
    /// С какой скоростью шёл на этом шаге, м/с (для анимации).
    pub speed: Speed,
}

/// Скорость, хешируемая по битам.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Speed(pub f32);

impl Hash for Speed {
    fn hash<H: Hasher>(&self, h: &mut H) {
        self.0.to_bits().hash(h);
    }
}

/// Снаряд в полёте (хак H-10: заранее известная дуга от `from` к `to`).
#[derive(Debug, Clone, Copy, PartialEq, Hash)]
pub struct Projectile {
    pub from: Vec2,
    pub to: Vec2,
    /// Сколько шагов уже летит и сколько всего будет лететь.
    pub age: u16,
    pub flight: u16,
    pub side: Side,
    pub attack: i32,
}

impl Projectile {
    /// Доля пройденного пути на момент `age + alpha`, от 0 до 1.
    pub fn progress(&self, alpha: f32) -> f32 {
        ((f32::from(self.age) + alpha) / f32::from(self.flight.max(1))).clamp(0.0, 1.0)
    }
}

/// Приказ, пришедший от игрока или ИИ.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Command {
    /// Идти в точку `dest`. Без `facing` отряд встанет лицом по ходу движения.
    Move {
        unit: UnitId,
        dest: Vec2,
        facing: Option<Vec2>,
        /// Сколько солдат в первой шеренге; без него ширина строя не меняется.
        width: Option<u32>,
        run: bool,
    },
    /// Атаковать отряд `target`.
    Attack {
        unit: UnitId,
        target: UnitId,
        run: bool,
    },
    /// Остановиться и стоять в строю.
    Halt { unit: UnitId },
}

/// Отряд в начальной расстановке.
#[derive(Debug, Clone, PartialEq)]
pub struct UnitSetup {
    /// Имя юнита из `export_descr_unit.txt` (`type`).
    pub unit: String,
    /// Центр первой шеренги.
    pub position: Vec2,
    pub facing: Vec2,
}

/// Армия одной стороны.
#[derive(Debug, Clone, PartialEq)]
pub struct ArmySetup {
    /// Фракция из `descr_sm_factions.txt` (цвета знамён).
    pub faction: String,
    pub units: Vec<UnitSetup>,
}

/// Всё, что задаёт битву: две армии и зерно случайности.
#[derive(Debug, Clone, PartialEq)]
pub struct BattleSetup {
    pub seed: u64,
    pub armies: [ArmySetup; 2],
}

/// Итог битвы.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Victory {
        winner: Side,
    },
    /// Обе армии бежали одновременно.
    Draw,
}

/// Ошибка в расстановке (например, юнита нет в данных).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetupError(pub String);

impl std::fmt::Display for SetupError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for SetupError {}

/// Битва целиком.
pub struct Battle {
    cfg: BattleConfig,
    seed: u64,
    tick: u64,
    world: World,
    units: Vec<Regiment>,
    factions: [String; 2],
    outcome: Option<Outcome>,
    /// Сколько солдат каждой стороны убито и сколько убежало с поля.
    pub killed: [u32; 2],
    pub fled: [u32; 2],
}

impl Battle {
    pub fn new(
        data: &GameData,
        setup: &BattleSetup,
        cfg: BattleConfig,
    ) -> Result<Self, SetupError> {
        let mut battle = Self {
            cfg,
            seed: setup.seed,
            tick: 0,
            world: World::new(),
            units: Vec::new(),
            factions: [
                setup.armies[0].faction.clone(),
                setup.armies[1].faction.clone(),
            ],
            outcome: None,
            killed: [0; 2],
            fled: [0; 2],
        };
        for (side, army) in setup.armies.iter().enumerate() {
            if data.faction(&army.faction).is_none() {
                return Err(SetupError(format!(
                    "фракция `{}` не найдена в данных",
                    army.faction
                )));
            }
            for u in &army.units {
                let unit = data.unit(&u.unit).ok_or_else(|| {
                    SetupError(format!(
                        "юнит `{}` не найден в export_descr_unit.txt",
                        u.unit
                    ))
                })?;
                battle.spawn_unit(unit, side as Side, u);
            }
        }
        if battle.units.len() > usize::from(u16::MAX) {
            return Err(SetupError("слишком много отрядов".into()));
        }
        Ok(battle)
    }

    fn spawn_unit(&mut self, unit: &rm_data::Unit, side: Side, setup: &UnitSetup) {
        let stats = UnitStats::from_data(unit, &self.cfg);
        let count = unit.soldier.count.clamp(1, 1000);
        let id = UnitId(self.units.len() as u16);
        let facing = setup.facing.normalize_or(Vec2::NORTH);
        let width = count.div_ceil(stats.ranks).max(1);
        let mut reg = Regiment {
            id,
            side,
            name: unit.name.clone(),
            initial_count: count,
            alive: count,
            ammo: 0,
            order: Order::Hold,
            state: UnitState::Ready,
            anchor: setup.position,
            facing,
            width,
            center: setup.position,
            morale: stats.morale,
            routing: false,
            rallied_once: false,
            charge_ticks: 0,
            run_distance: 0.0,
            in_melee: false,
            slots_dirty: false,
            casualties_tick: 0,
            rear_hits_tick: 0,
            stats,
        };
        let mut rng = Rng::for_entity(self.seed, RngStream::COMBAT, 0, u64::from(id.0));
        let ammo = reg.stats.missile.as_ref().map_or(0, |m| m.ammo);
        reg.ammo = u32::from(ammo) * count;
        let soldiers: Vec<(Soldier, Body)> = (0..count)
            .map(|slot| {
                // Небольшой разброс, чтобы строй не выглядел как по линейке.
                let jitter = Vec2::new(rng.next_f32() - 0.5, rng.next_f32() - 0.5) * 0.3;
                let pos = setup.position + reg.slot_offset(slot) + jitter;
                (
                    Soldier {
                        unit: id,
                        side,
                        slot: slot as u16,
                        hp: reg.stats.hit_points,
                        cooldown: rng.below(reg.stats.melee_delay_ticks.max(1)) as u16,
                        ammo,
                        since_strike: u8::MAX,
                        fighting: false,
                    },
                    Body {
                        pos,
                        prev: pos,
                        facing,
                        speed: Speed(0.0),
                    },
                )
            })
            .collect();
        self.world.spawn_batch(soldiers);
        self.units.push(reg);
    }

    pub fn config(&self) -> &BattleConfig {
        &self.cfg
    }

    pub fn seed(&self) -> u64 {
        self.seed
    }

    /// Номер последнего выполненного шага.
    pub fn tick(&self) -> u64 {
        self.tick
    }

    /// Игровое время с начала битвы, секунды.
    pub fn time_seconds(&self) -> f32 {
        self.tick as f32 * self.cfg.tick_seconds
    }

    pub fn units(&self) -> &[Regiment] {
        &self.units
    }

    pub fn unit(&self, id: UnitId) -> Option<&Regiment> {
        self.units.get(id.index())
    }

    pub fn faction(&self, side: Side) -> &str {
        &self.factions[usize::from(side.min(1))]
    }

    pub fn outcome(&self) -> Option<Outcome> {
        self.outcome
    }

    /// Все живые солдаты.
    pub fn soldiers(&self) -> impl Iterator<Item = (&Soldier, &Body)> {
        self.world.query::<(&Soldier, &Body)>()
    }

    /// Все снаряды в полёте.
    pub fn projectiles(&self) -> impl Iterator<Item = &Projectile> {
        self.world.query::<&Projectile>()
    }

    /// Живых солдат у стороны.
    pub fn alive(&self, side: Side) -> u32 {
        self.units
            .iter()
            .filter(|u| u.side == side)
            .map(|u| u.alive)
            .sum()
    }

    /// Хеш всего состояния битвы — контрольная точка для реплеев.
    pub fn state_hash(&self) -> u64 {
        let mut h = StableHasher::new();
        h.write_u64(self.tick);
        h.write_u64(self.world.hash_component::<Soldier>());
        h.write_u64(self.world.hash_component::<Body>());
        h.write_u64(self.world.hash_component::<Projectile>());
        for u in &self.units {
            u.alive.hash(&mut h);
            u.morale.to_bits().hash(&mut h);
            u.routing.hash(&mut h);
            u.center.hash(&mut h);
        }
        h.finish()
    }

    fn apply(&mut self, cmd: &Command) {
        let unit = match *cmd {
            Command::Move { unit, .. } | Command::Attack { unit, .. } | Command::Halt { unit } => {
                unit
            }
        };
        let Some(r) = self.units.get(unit.index()) else {
            return;
        };
        if !r.is_fighting() {
            return;
        }
        let center = r.center;
        let facing = r.facing;
        let order = match *cmd {
            Command::Move {
                dest,
                facing: f,
                run,
                width,
                ..
            } => {
                if let Some(w) = width {
                    let r = &mut self.units[unit.index()];
                    r.width = w.clamp(1, r.alive.max(1));
                    r.slots_dirty = true;
                }
                let f = f.unwrap_or(dest - center).normalize_or(facing);
                Order::Move {
                    dest,
                    facing: f,
                    run,
                }
            }
            Command::Attack { target, run, .. } => {
                let valid = self
                    .units
                    .get(target.index())
                    .is_some_and(|t| t.is_active() && t.side != r.side);
                if !valid {
                    return;
                }
                Order::Attack { target, run }
            }
            Command::Halt { .. } => Order::Hold,
        };
        let r = &mut self.units[unit.index()];
        if matches!(order, Order::Hold) {
            r.anchor = r.center;
        }
        if !matches!(r.order, Order::Attack { .. }) || !matches!(order, Order::Attack { .. }) {
            r.run_distance = 0.0;
        }
        r.order = order;
    }
}

#[cfg(test)]
mod tests;
