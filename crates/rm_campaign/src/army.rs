//! Армии: переходы между регионами, автобой и захват поселений.
//!
//! Правила базовой версии:
//!
//! * за ход армия делает `army_moves_per_turn` переходов в соседний регион,
//!   армия только из конницы — `cavalry_army_moves_per_turn`;
//! * дипломатии пока нет: все фракции враждуют со всеми;
//! * если в регионе назначения стоят чужие армии, с каждой по очереди
//!   проходит автобой. Проиграл нападающий — его армия уничтожена и
//!   никуда не идёт. Победил — армия защитника уничтожена, нападающий
//!   теряет часть солдат;
//! * если после этого в регионе нет защитников, а поселение чужое, оно
//!   переходит к нападающему. Осад пока нет.

use rm_core::Entity;
use rm_core::sim::{Rng, RngStream, random_u64};

use crate::Campaign;
use crate::command::{CampaignEvent, CommandError};
use crate::components::{Army, Location, Moves, Owner, RecruitQueue};
use crate::ids::{FactionId, RegionId};

impl Campaign {
    /// Сколько переходов за ход положено армии из этих отрядов.
    pub(crate) fn moves_for(&self, army: &Army) -> u8 {
        let all_cavalry =
            !army.units.is_empty() && army.units.iter().all(|u| self.unit_type(u.unit).cavalry);
        let moves = if all_cavalry {
            self.rules.cavalry_army_moves_per_turn
        } else {
            self.rules.army_moves_per_turn
        };
        moves.min(u32::from(u8::MAX)) as u8
    }

    /// Восстанавливает переходы армий фракции в начале её хода.
    pub(crate) fn reset_moves(&mut self, faction: FactionId) {
        let armies: Vec<(Entity, u8)> = self
            .world
            .query::<(Entity, &Army, &Owner)>()
            .filter(|(_, _, o)| o.0 == faction)
            .map(|(e, a, _)| (e, self.moves_for(a)))
            .collect();
        for (e, m) in armies {
            self.world.get_mut::<Moves>(e).expect("армия").0 = m;
        }
    }

    /// Команда перехода в соседний регион.
    pub(crate) fn move_army(&mut self, army: Entity, to: RegionId) -> Result<(), CommandError> {
        let faction = self.current;
        let (Some(owner), Some(from), Some(moves)) = (
            self.world.get::<Owner>(army).copied(),
            self.world.get::<Location>(army).copied(),
            self.world.get::<Moves>(army).copied(),
        ) else {
            return Err(CommandError::NoSuchArmy);
        };
        if owner.0 != faction {
            return Err(CommandError::NotYourArmy);
        }
        if to.index() >= self.map.regions().len() || !self.map.are_neighbours(from.0, to) {
            return Err(CommandError::NotAdjacent);
        }
        if moves.0 == 0 {
            return Err(CommandError::NoMovesLeft);
        }

        // Чужие армии в регионе назначения, в порядке ECS.
        let defenders: Vec<Entity> = self
            .world
            .query::<(Entity, &Army, &Owner, &Location)>()
            .filter(|(_, _, o, l)| o.0 != faction && l.0 == to)
            .map(|(e, _, _, _)| e)
            .collect();
        for defender in defenders {
            if !self.world.contains(army) {
                break;
            }
            if !self.battle(army, defender, to) {
                self.check_eliminations();
                return Ok(());
            }
        }

        if !self.world.contains(army) {
            // Нападающий победил, но потерял всех солдат.
            self.check_eliminations();
            return Ok(());
        }
        self.world.get_mut::<Location>(army).expect("армия").0 = to;
        self.world.get_mut::<Moves>(army).expect("армия").0 = moves.0 - 1;
        self.events.push(CampaignEvent::ArmyMoved {
            army,
            from: from.0,
            to,
        });

        if let Some(settlement) = self.settlements[to.index()] {
            let prev = self
                .world
                .get::<Owner>(settlement)
                .copied()
                .expect("поселение");
            if prev.0 != faction {
                *self.world.get_mut::<Owner>(settlement).expect("поселение") = Owner(faction);
                // Очередь найма прежнего владельца пропадает вместе с деньгами.
                self.world
                    .get_mut::<RecruitQueue>(settlement)
                    .expect("поселение")
                    .0
                    .clear();
                self.events.push(CampaignEvent::SettlementCaptured {
                    region: to,
                    from: prev.0,
                    to: faction,
                });
            }
        }
        self.check_eliminations();
        Ok(())
    }

    /// Автобой. Возвращает `true`, если нападающий победил.
    ///
    /// Сила стороны — сумма `солдаты × сила солдата` по отрядам, умноженная
    /// на удачу `100 ± autoresolve_spread_percent` процентов. Ничья — победа
    /// защитника. Проигравшая армия уничтожается. Победитель теряет в каждом
    /// отряде долю солдат `сила проигравшего / сила победителя ×
    /// autoresolve_winner_loss_percent / 100`.
    fn battle(&mut self, attacker: Entity, defender: Entity, region: RegionId) -> bool {
        let att_owner = self.world.get::<Owner>(attacker).expect("армия").0;
        let def_owner = self.world.get::<Owner>(defender).expect("армия").0;
        let att_base = self.army_power(&self.world.get::<Army>(attacker).expect("армия").units);
        let def_base = self.army_power(&self.world.get::<Army>(defender).expect("армия").units);

        let mut rng = Rng::new(random_u64(
            self.seed,
            RngStream::CAMPAIGN_EVENTS,
            self.battles,
            attacker.to_bits(),
            defender.to_bits(),
        ));
        self.battles += 1;
        let spread = self.rules.autoresolve_spread_percent;
        let mut luck =
            |power: u64| power * u64::from(100 - spread + rng.below(2 * spread + 1)) / 100;
        let att_power = luck(att_base);
        let def_power = luck(def_base);
        let attacker_won = att_power > def_power;
        self.events.push(CampaignEvent::Battle {
            region,
            attacker: att_owner,
            defender: def_owner,
            attacker_won,
            attacker_power: att_power,
            defender_power: def_power,
        });

        let (winner, loser, win_power, lose_power) = if attacker_won {
            (attacker, defender, att_power, def_power)
        } else {
            (defender, attacker, def_power, att_power)
        };
        self.destroy_army(loser);
        let loss_pct = u64::from(self.rules.autoresolve_winner_loss_percent);
        let denom = win_power.max(1) * 100;
        let army = self.world.get_mut::<Army>(winner).expect("армия");
        for u in &mut army.units {
            let lost = u64::from(u.soldiers) * lose_power * loss_pct / denom;
            u.soldiers = u
                .soldiers
                .saturating_sub(lost.min(u64::from(u32::MAX)) as u32);
        }
        army.units.retain(|u| u.soldiers > 0);
        if army.units.is_empty() {
            self.destroy_army(winner);
        }
        attacker_won
    }

    fn destroy_army(&mut self, army: Entity) {
        let faction = self.world.get::<Owner>(army).expect("армия").0;
        self.world.despawn(army);
        self.events
            .push(CampaignEvent::ArmyDestroyed { army, faction });
    }
}
