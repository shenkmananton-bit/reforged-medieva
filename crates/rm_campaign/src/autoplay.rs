//! Простейший «автопилот» для фракции: заглушка до настоящего ИИ кампании
//! (`rm_campaign_ai`, агент D6).
//!
//! Нужен, чтобы кампанию можно было прогнать без игрока: в тестах, в
//! примере `headless` и для проверки детерминизма. Решения грубые:
//!
//! 1. **Найм.** В каждом своём поселении с пустой очередью нанимает самый
//!    сильный доступный юнит, если после покупки в казне останется запас
//!    на два хода содержания армий.
//! 2. **Поход.** Каждая армия идёт по кратчайшему пути к ближайшему чужому
//!    региону. Шаг делается, только если армия сильнее всех чужих армий в
//!    следующем регионе вместе взятых хотя бы в полтора раза.
//!
//! Автопилот меняет кампанию только через [`Campaign::apply`] и
//! возвращает список отданных команд — по нему ход можно повторить.

use crate::Campaign;
use crate::command::Command;
use crate::ids::RegionId;

/// Отдаёт команды за текущую фракцию. Ход не заканчивает: после вызова
/// нужен [`Campaign::end_turn`]. Возвращает выполненные команды.
pub fn play_turn(campaign: &mut Campaign) -> Vec<Command> {
    let mut done = Vec::new();
    recruit(campaign, &mut done);
    advance(campaign, &mut done);
    done
}

fn recruit(c: &mut Campaign, done: &mut Vec<Command>) {
    let faction = c.current_faction();
    let regions: Vec<RegionId> = c
        .settlements()
        .into_iter()
        .filter(|s| s.owner == faction && s.queue.is_empty())
        .map(|s| s.region)
        .collect();
    for region in regions {
        let reserve = c.upkeep(faction) * 2;
        let treasury = c.faction(faction).treasury;
        let best = c
            .unit_types()
            .iter()
            .filter(|u| u.ownership.contains(&faction) && treasury - u.price >= reserve)
            .max_by_key(|u| {
                (
                    u64::from(u.soldiers) * u.power_per_soldier,
                    std::cmp::Reverse(u.id),
                )
            })
            .map(|u| u.id);
        if let Some(unit) = best {
            let cmd = Command::Recruit { region, unit };
            if c.apply(cmd).is_ok() {
                done.push(cmd);
            }
        }
    }
}

fn advance(c: &mut Campaign, done: &mut Vec<Command>) {
    let faction = c.current_faction();
    let armies: Vec<_> = c
        .armies()
        .into_iter()
        .filter(|a| a.owner == faction)
        .map(|a| a.entity)
        .collect();
    for army in armies {
        while let Some(view) = c.army(army) {
            if view.moves == 0 {
                break;
            }
            let foreign = |r: RegionId| c.settlement_in(r).is_some_and(|s| s.owner != faction);
            let Some(path) = c.map().path_to(view.region, foreign) else {
                break;
            };
            let Some(&next) = path.get(1) else { break };
            let enemy: u64 = c
                .armies()
                .iter()
                .filter(|a| a.owner != faction && a.region == next)
                .map(|a| c.army_power(&a.units))
                .sum();
            let ours = c.army_power(&view.units);
            if ours * 2 < enemy * 3 {
                break;
            }
            let cmd = Command::MoveArmy { army, to: next };
            if c.apply(cmd).is_err() {
                break;
            }
            done.push(cmd);
        }
    }
}
