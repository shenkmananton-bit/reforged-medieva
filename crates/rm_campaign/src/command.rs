//! Команды и события кампании.
//!
//! По правилам детерминизма (`docs/adr/determinism.md`) состояние кампании
//! меняется только командами. Команду отдаёт фракция, которая сейчас ходит:
//! игрок через интерфейс или ИИ. Всё, что произошло в ответ, кампания
//! складывает в список [`CampaignEvent`] — его читают интерфейс, лог и тесты.

use rm_core::Entity;

use crate::ids::{FactionId, RegionId, UnitTypeId};

/// Приказ от фракции, которая сейчас ходит.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Command {
    /// Перевести армию в соседний регион. Если там чужие армии — автобой;
    /// если там чужое поселение без защитников — захват.
    MoveArmy { army: Entity, to: RegionId },
    /// Поставить отряд в очередь найма поселения. Деньги списываются сразу.
    Recruit { region: RegionId, unit: UnitTypeId },
}

/// Почему команда не выполнена. Состояние при этом не меняется.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandError {
    /// Кампания закончилась.
    CampaignOver,
    /// Такой армии нет (уничтожена или не существовала).
    NoSuchArmy,
    /// Армия принадлежит другой фракции.
    NotYourArmy,
    /// Регион не граничит с регионом армии.
    NotAdjacent,
    /// Армия уже израсходовала переходы на этот ход.
    NoMovesLeft,
    /// В регионе нет поселения или оно чужое.
    NotYourSettlement,
    /// Фракция не может нанимать этот юнит (`ownership`).
    UnitNotAvailable,
    /// Не хватает денег.
    NotEnoughMoney { need: i64, have: i64 },
}

impl std::fmt::Display for CommandError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CampaignOver => write!(f, "кампания закончилась"),
            Self::NoSuchArmy => write!(f, "такой армии нет"),
            Self::NotYourArmy => write!(f, "это чужая армия"),
            Self::NotAdjacent => write!(f, "регион не граничит с регионом армии"),
            Self::NoMovesLeft => write!(f, "армия уже походила в этот ход"),
            Self::NotYourSettlement => write!(f, "в регионе нет вашего поселения"),
            Self::UnitNotAvailable => write!(f, "фракция не может нанимать этот юнит"),
            Self::NotEnoughMoney { need, have } => {
                write!(f, "не хватает денег: нужно {need}, в казне {have}")
            }
        }
    }
}

impl std::error::Error for CommandError {}

/// Доходы и расходы фракции за один ход.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct IncomeReport {
    /// Налоги с населения.
    pub tax: i64,
    /// Доход с ферм.
    pub farms: i64,
    /// Торговля ресурсами.
    pub trade: i64,
    /// Кошелёк короля.
    pub kings_purse: i64,
    /// Содержание армий (положительное число, вычитается).
    pub upkeep: i64,
}

impl IncomeReport {
    /// Итог за ход: доходы минус содержание.
    pub fn net(&self) -> i64 {
        self.tax + self.farms + self.trade + self.kings_purse - self.upkeep
    }
}

/// Что произошло в кампании.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CampaignEvent {
    /// Начался ход фракции.
    TurnStarted { turn: u32, faction: FactionId },
    /// Фракция получила доход и заплатила за армии.
    Income {
        faction: FactionId,
        report: IncomeReport,
        treasury: i64,
    },
    /// Отряд обучен и встал в армию в регионе поселения.
    UnitRecruited {
        faction: FactionId,
        region: RegionId,
        unit: UnitTypeId,
        army: Entity,
    },
    /// Армия перешла в соседний регион.
    ArmyMoved {
        army: Entity,
        from: RegionId,
        to: RegionId,
    },
    /// Автобой между армиями.
    Battle {
        region: RegionId,
        attacker: FactionId,
        defender: FactionId,
        attacker_won: bool,
        /// Сила сторон после броска удачи.
        attacker_power: u64,
        defender_power: u64,
    },
    /// Армия уничтожена.
    ArmyDestroyed { army: Entity, faction: FactionId },
    /// Поселение сменило владельца.
    SettlementCaptured {
        region: RegionId,
        from: FactionId,
        to: FactionId,
    },
    /// У фракции не осталось ни поселений, ни армий.
    FactionDestroyed { faction: FactionId },
}
