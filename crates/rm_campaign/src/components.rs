//! Компоненты ECS кампании.
//!
//! Поселения и армии — сущности в [`rm_core::World`]. У поселения есть
//! [`Settlement`], [`Owner`] и [`RecruitQueue`]; у армии — [`Army`],
//! [`Owner`], [`Location`] и [`Moves`]. Регионы и фракции в ECS не живут:
//! регионов мало и они не меняются (это [`CampaignMap`](crate::CampaignMap)),
//! а фракции — маленькая таблица [`FactionState`](crate::FactionState).

use crate::ids::{FactionId, RegionId, UnitTypeId};

/// Чья это сущность.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Owner(pub FactionId);

/// В каком регионе стоит армия.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Location(pub RegionId);

/// Сколько переходов между регионами армия ещё может сделать в этот ход.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Moves(pub u8);

/// Поселение — столица региона.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Settlement {
    pub region: RegionId,
    /// Номер уровня в `settlement_level` из `descr_rm_campaign.txt`.
    pub level: u8,
    pub population: u32,
}

/// Отряд, который сейчас обучается в поселении.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Recruit {
    pub unit: UnitTypeId,
    /// Сколько ходов (своих начал хода) осталось до готовности.
    pub turns_left: u32,
}

/// Очередь найма поселения. Обучаются первые `recruit_slots_per_turn` отрядов.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct RecruitQueue(pub Vec<Recruit>);

/// Отряд в армии.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ArmyUnit {
    pub unit: UnitTypeId,
    /// Сколько солдат осталось.
    pub soldiers: u32,
    /// Опыт (`exp` в `descr_strat.txt`).
    pub exp: u32,
}

/// Армия: полководец (если есть) и отряды.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Army {
    pub general: Option<String>,
    pub units: Vec<ArmyUnit>,
}
