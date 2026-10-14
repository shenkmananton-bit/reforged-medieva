use std::path::Path;

use rm_battle::{ArmySetup, Battle, BattleConfig, BattleSetup, Outcome, UnitSetup, Vec2};
use rm_data::GameData;

use super::*;

fn data() -> GameData {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data");
    GameData::load_dir(&dir).expect("data/ разбирается")
}

fn army(faction: &str, units: &[&str], y: f32, facing: Vec2) -> ArmySetup {
    let n = units.len() as f32;
    ArmySetup {
        faction: faction.into(),
        units: units
            .iter()
            .enumerate()
            .map(|(i, u)| UnitSetup {
                unit: (*u).into(),
                position: Vec2::new((i as f32 - (n - 1.0) / 2.0) * 35.0, y),
                facing,
            })
            .collect(),
    }
}

fn setup(seed: u64) -> BattleSetup {
    BattleSetup {
        seed,
        armies: [
            army(
                "northmark",
                &[
                    "Village Levy",
                    "Shield Wall Spearmen",
                    "Crossbow Militia",
                    "Village Levy",
                ],
                -150.0,
                Vec2::NORTH,
            ),
            army(
                "southreach",
                &[
                    "Village Levy",
                    "Crossbow Militia",
                    "Steppe Horse Archers",
                    "Village Levy",
                ],
                150.0,
                -Vec2::NORTH,
            ),
        ],
    }
}

/// Прогоняет битву ИИ против ИИ, возвращает итог, время и контрольные хеши.
fn run(seed: u64) -> (Option<Outcome>, f32, Vec<u64>) {
    let mut b = Battle::new(&data(), &setup(seed), BattleConfig::default()).unwrap();
    let mut ai = [BattleAi::new(0), BattleAi::new(1)];
    let mut hashes = Vec::new();
    for _ in 0..20 * 60 * 10 {
        let mut cmds = ai[0].think(&b);
        cmds.extend(ai[1].think(&b));
        b.step(&cmds);
        if b.tick().is_multiple_of(20) {
            hashes.push(b.state_hash());
        }
        if b.outcome().is_some() {
            break;
        }
    }
    (b.outcome(), b.time_seconds(), hashes)
}

#[test]
fn ai_battle_finishes() {
    let (outcome, time, _) = run(11);
    assert!(outcome.is_some(), "ИИ против ИИ не закончили за 10 минут");
    assert!(time > 20.0, "слишком быстро: {time} с");
}

#[test]
fn ai_is_deterministic() {
    assert_eq!(run(5).2, run(5).2);
}

#[test]
fn ai_marches_toward_enemy() {
    let mut b = Battle::new(&data(), &setup(1), BattleConfig::default()).unwrap();
    let mut ai = BattleAi::new(1);
    let start = b.units()[4].center;
    for _ in 0..20 * 10 {
        let cmds = ai.think(&b);
        b.step(&cmds);
    }
    let now = b.units()[4].center;
    assert!(
        now.y < start.y - 10.0,
        "армия севера не пошла на юг: {start:?} → {now:?}"
    );
}

#[test]
#[ignore]
fn trace() {
    for seed in 1..6 {
        let (o, t, _) = run(seed);
        println!("seed {seed}: {o:?} за {t:.0} с");
    }
}

#[test]
#[ignore]
fn trace_one() {
    let seed: u64 = std::env::var("SEED")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(11);
    let mut b = Battle::new(&data(), &setup(seed), BattleConfig::default()).unwrap();
    let mut ai = [BattleAi::new(0), BattleAi::new(1)];
    for t in 0..20 * 60 * 10 {
        let mut cmds = ai[0].think(&b);
        cmds.extend(ai[1].think(&b));
        b.step(&cmds);
        if t % 400 == 0 {
            let s: Vec<String> = b
                .units()
                .iter()
                .map(|u| {
                    format!(
                        "{}:{:?} n={} a={} m={:.0} ({:.0},{:.0}) {:?}",
                        u.id.0, u.state, u.alive, u.ammo, u.morale, u.center.x, u.center.y, u.order
                    )
                })
                .collect();
            println!("t={:.0}s\n  {}", b.time_seconds(), s.join("\n  "));
        }
        if b.outcome().is_some() {
            break;
        }
    }
}

/// Зеркальные армии: победы должны делиться примерно поровну (нет перекоса в пользу стороны 0).
#[test]
#[ignore]
fn mirror() {
    let units = [
        "Village Levy",
        "Shield Wall Spearmen",
        "Crossbow Militia",
        "Village Levy",
    ];
    let mut wins = [0, 0, 0];
    for seed in 1..13 {
        let s = BattleSetup {
            seed,
            armies: [
                army("northmark", &units, -120.0, Vec2::NORTH),
                army("southreach", &units, 120.0, -Vec2::NORTH),
            ],
        };
        let mut b = Battle::new(&data(), &s, BattleConfig::default()).unwrap();
        let mut ai = [BattleAi::new(0), BattleAi::new(1)];
        for _ in 0..20 * 600 {
            let mut cmds = ai[0].think(&b);
            cmds.extend(ai[1].think(&b));
            b.step(&cmds);
            if b.outcome().is_some() {
                break;
            }
        }
        match b.outcome() {
            Some(Outcome::Victory { winner }) => wins[winner as usize] += 1,
            _ => wins[2] += 1,
        }
        println!("seed {seed}: {:?} killed {:?}", b.outcome(), b.killed);
    }
    println!("wins {wins:?}");
}
