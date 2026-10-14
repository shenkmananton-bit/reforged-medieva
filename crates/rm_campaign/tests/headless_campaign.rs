//! Кампания из настоящей папки `data/` без окна: несколько десятков ходов
//! на автопилоте, проверка правил по ходу игры и повтор по списку команд.

use std::path::PathBuf;

use rm_campaign::{Campaign, CampaignEvent, Command, autoplay};
use rm_data::CampaignData;

fn data() -> CampaignData {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data");
    CampaignData::load_dir(&dir).unwrap_or_else(|e| panic!("data/ не разбирается:\n{e}"))
}

/// Один ход фракции: какие команды она отдала и хеш после конца хода.
struct Step {
    commands: Vec<Command>,
    hash: u64,
}

const TURNS: u32 = 30;
const SEED: u64 = 2026;

fn play(data: &CampaignData) -> (Campaign, Vec<Step>, Vec<CampaignEvent>) {
    let mut c = Campaign::new(data, SEED).unwrap();
    let mut steps = Vec::new();
    let mut events = Vec::new();
    while c.turn() < TURNS && !c.is_finished() {
        let commands = autoplay::play_turn(&mut c);
        c.end_turn();
        steps.push(Step {
            commands,
            hash: c.state_hash(),
        });
        events.extend(c.drain_events());
    }
    (c, steps, events)
}

#[test]
fn first_turn_income_matches_hand_calculation() {
    let c = Campaign::new(&data(), SEED).unwrap();
    let north = c.faction_id("northmark").unwrap();
    assert_eq!(c.current_faction(), north);
    let r = c.faction(north).last_income;
    // Ostmark: 2400 жителей, плодородие 3 → прирост 0,5% + 3 × 0,2% = 1,1% → 2426.
    // Westfold: 800, плодородие 2 → 0,9% → 807.
    // Налоги: 2426 × 3 / 100 + 807 × 3 / 100 = 72 + 24.
    assert_eq!(r.tax, 72 + 24);
    // Фермы: (3 + 2) × 30.
    assert_eq!(r.farms, 150);
    // Торговля: железо 40 + меха 35 (Ostmark) + лес 30 (Westfold).
    assert_eq!(r.trade, 105);
    assert_eq!(r.kings_purse, 1200);
    // Содержание: 2 × копейщики 140 + 3 × ополчение 60.
    assert_eq!(r.upkeep, 2 * 140 + 3 * 60);
    assert_eq!(c.faction(north).treasury, 4000 + r.net());
}

#[test]
fn thirty_turns_on_autopilot_keep_the_rules() {
    let data = data();
    let (c, steps, events) = play(&data);
    assert!(c.turn() >= 10, "кампания закончилась слишком рано");

    // Доход начислялся в каждый ход каждой живой фракции.
    let incomes = events
        .iter()
        .filter(|e| matches!(e, CampaignEvent::Income { .. }))
        .count();
    assert!(incomes >= 30);

    // Армии не бывают пустыми, поселения принадлежат живым фракциям.
    for a in c.armies() {
        assert!(!a.units.is_empty());
        assert!(a.units.iter().all(|u| u.soldiers > 0));
        assert!(c.faction(a.owner).alive);
    }
    for s in c.settlements() {
        assert!(c.faction(s.owner).alive);
        let cap = c.rules().settlement_levels[s.level as usize].population_cap;
        assert!(s.population <= cap);
    }

    // За 30 ходов автопилот хоть что-то нанял, сходил и захватил.
    let count = |f: fn(&CampaignEvent) -> bool| events.iter().filter(|e| f(e)).count();
    assert!(count(|e| matches!(e, CampaignEvent::UnitRecruited { .. })) > 0);
    assert!(count(|e| matches!(e, CampaignEvent::ArmyMoved { .. })) > 0);
    assert!(count(|e| matches!(e, CampaignEvent::SettlementCaptured { .. })) > 0);
    assert!(steps.iter().any(|s| !s.commands.is_empty()));
}

#[test]
fn replay_from_commands_gives_the_same_hashes() {
    let data = data();
    let (_, steps, _) = play(&data);
    let mut c = Campaign::new(&data, SEED).unwrap();
    for (i, step) in steps.iter().enumerate() {
        for &cmd in &step.commands {
            c.apply(cmd)
                .unwrap_or_else(|e| panic!("шаг {i}: команда {cmd:?} не прошла при повторе: {e}"));
        }
        c.end_turn();
        assert_eq!(c.state_hash(), step.hash, "расхождение после шага {i}");
    }
}

#[test]
fn different_seeds_are_still_valid_games() {
    let data = data();
    for seed in [1, 2, 3, 99] {
        let mut c = Campaign::new(&data, seed).unwrap();
        while c.turn() < 15 && !c.is_finished() {
            autoplay::play_turn(&mut c);
            c.end_turn();
        }
        assert!(c.factions().iter().any(|f| f.alive));
    }
}
