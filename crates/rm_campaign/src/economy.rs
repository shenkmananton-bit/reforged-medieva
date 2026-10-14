//! Экономика: найм, рост населения, доход и содержание армий.
//!
//! Всё считается в начале хода фракции (как в Medieval II) и только целыми
//! числами, чтобы результат был одинаков на любой машине.

use rm_core::Entity;

use crate::Campaign;
use crate::command::{CampaignEvent, CommandError, IncomeReport};
use crate::components::{
    Army, ArmyUnit, Location, Moves, Owner, Recruit, RecruitQueue, Settlement,
};
use crate::ids::{FactionId, RegionId, UnitTypeId};

impl Campaign {
    /// Поселения фракции в порядке ECS.
    fn settlements_of(&self, faction: FactionId) -> Vec<Entity> {
        self.world
            .query::<(Entity, &Settlement, &Owner)>()
            .filter(|(_, _, o)| o.0 == faction)
            .map(|(e, _, _)| e)
            .collect()
    }

    /// Команда найма: проверки, списание денег, постановка в очередь.
    pub(crate) fn recruit(
        &mut self,
        region: RegionId,
        unit: UnitTypeId,
    ) -> Result<(), CommandError> {
        let faction = self.current;
        let settlement = self
            .settlements
            .get(region.index())
            .copied()
            .flatten()
            .filter(|&e| self.world.get::<Owner>(e) == Some(&Owner(faction)))
            .ok_or(CommandError::NotYourSettlement)?;
        let kind = self
            .units
            .get(unit.index())
            .ok_or(CommandError::UnitNotAvailable)?;
        if !kind.ownership.contains(&faction) {
            return Err(CommandError::UnitNotAvailable);
        }
        let price = kind.price;
        let turns = kind.recruit_turns.max(1);
        let have = self.factions[faction.index()].treasury;
        if have < price {
            return Err(CommandError::NotEnoughMoney { need: price, have });
        }
        self.factions[faction.index()].treasury -= price;
        self.world
            .get_mut::<RecruitQueue>(settlement)
            .expect("у поселения есть очередь")
            .0
            .push(Recruit {
                unit,
                turns_left: turns,
            });
        Ok(())
    }

    /// Продвигает очереди найма поселений фракции. Готовый отряд встаёт
    /// в первую армию фракции в этом регионе, а если её нет — в новую армию
    /// без полководца (гарнизон).
    pub(crate) fn progress_recruitment(&mut self, faction: FactionId) {
        let slots = self.rules.recruit_slots_per_turn as usize;
        for e in self.settlements_of(faction) {
            let region = self.world.get::<Settlement>(e).expect("поселение").region;
            let queue = &mut self.world.get_mut::<RecruitQueue>(e).expect("очередь").0;
            let mut ready = Vec::new();
            for r in queue.iter_mut().take(slots) {
                r.turns_left = r.turns_left.saturating_sub(1);
                if r.turns_left == 0 {
                    ready.push(r.unit);
                }
            }
            queue.retain(|r| r.turns_left > 0);
            for unit in ready {
                let army = self.garrison(faction, region);
                let soldiers = self.units[unit.index()].soldiers;
                self.world
                    .get_mut::<Army>(army)
                    .expect("армия")
                    .units
                    .push(ArmyUnit {
                        unit,
                        soldiers,
                        exp: 0,
                    });
                self.events.push(CampaignEvent::UnitRecruited {
                    faction,
                    region,
                    unit,
                    army,
                });
            }
        }
    }

    /// Первая армия фракции в регионе; если её нет — создаёт пустую.
    fn garrison(&mut self, faction: FactionId, region: RegionId) -> Entity {
        let existing = self
            .world
            .query::<(Entity, &Owner, &Location)>()
            .find(|(_, o, l)| o.0 == faction && l.0 == region)
            .map(|(e, _, _)| e);
        existing.unwrap_or_else(|| {
            self.world.spawn((
                Army {
                    general: None,
                    units: Vec::new(),
                },
                Owner(faction),
                Location(region),
                Moves(0),
            ))
        })
    }

    /// Рост населения: `население × (база + плодородие × добавка) / 1000`,
    /// но не выше предела уровня поселения.
    pub(crate) fn grow_population(&mut self, faction: FactionId) {
        let base = u64::from(self.rules.growth_base_permille);
        let per_farm = u64::from(self.rules.growth_per_farm_level_permille);
        for e in self.settlements_of(faction) {
            let s = *self.world.get::<Settlement>(e).expect("поселение");
            let farming = u64::from(self.map.region(s.region).farming_level);
            let cap = self.rules.settlement_levels[s.level as usize].population_cap;
            let pop = u64::from(s.population);
            let grown = pop + pop * (base + farming * per_farm) / 1000;
            self.world
                .get_mut::<Settlement>(e)
                .expect("поселение")
                .population = grown.min(u64::from(cap)) as u32;
        }
    }

    /// Доход фракции за ход без изменения состояния: налоги, фермы,
    /// торговля, кошелёк короля и содержание армий.
    pub fn income_report(&self, faction: FactionId) -> IncomeReport {
        let mut report = IncomeReport {
            kings_purse: self.factions[faction.index()].kings_purse,
            upkeep: self.upkeep(faction),
            ..IncomeReport::default()
        };
        for (s, o) in self.world.query::<(&Settlement, &Owner)>() {
            if o.0 != faction {
                continue;
            }
            let region = self.map.region(s.region);
            report.tax +=
                i64::from(s.population) * i64::from(self.rules.tax_per_100_population) / 100;
            report.farms +=
                i64::from(region.farming_level) * i64::from(self.rules.farm_income_per_level);
            report.trade += i64::from(region.trade_value);
        }
        report
    }

    /// Начисляет доход за ход и списывает содержание.
    pub(crate) fn collect_income(&mut self, faction: FactionId) {
        let report = self.income_report(faction);
        let f = &mut self.factions[faction.index()];
        f.treasury += report.net();
        f.last_income = report;
        self.events.push(CampaignEvent::Income {
            faction,
            report,
            treasury: f.treasury,
        });
    }
}
