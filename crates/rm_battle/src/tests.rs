//! Тесты битвы целиком, без окна и видеокарты.

use std::path::Path;

use rm_data::GameData;

use super::*;

pub(crate) fn data() -> GameData {
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
                position: Vec2::new((i as f32 - (n - 1.0) / 2.0) * 30.0, y),
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
                &["Shield Wall Spearmen", "Village Levy"],
                -40.0,
                Vec2::NORTH,
            ),
            army(
                "southreach",
                &["Village Levy", "Crossbow Militia"],
                40.0,
                -Vec2::NORTH,
            ),
        ],
    }
}

/// Простейший «ИИ» для тестов: каждый отряд атакует ближайшего врага.
fn charge_all(b: &Battle) -> Vec<Command> {
    b.units()
        .iter()
        .filter(|u| u.is_fighting() && !matches!(u.order, Order::Attack { .. }))
        .filter_map(|u| {
            b.nearest_enemy_unit(u.id.index(), f32::MAX)
                .map(|target| Command::Attack {
                    unit: u.id,
                    target,
                    run: false,
                })
        })
        .collect()
}

#[test]
fn units_spawn_from_data() {
    let b = Battle::new(&data(), &setup(1), BattleConfig::default()).unwrap();
    assert_eq!(b.units().len(), 4);
    let spears = &b.units()[0];
    assert_eq!(spears.name, "Shield Wall Spearmen");
    assert_eq!(spears.alive, 60);
    assert_eq!(b.soldiers().count(), 60 + 60 + 60 + 48);
    assert!(b.units()[3].stats.missile.is_some());
}

#[test]
fn unknown_unit_is_an_error() {
    let mut s = setup(1);
    s.armies[0].units[0].unit = "Dragon".into();
    let err = Battle::new(&data(), &s, BattleConfig::default())
        .err()
        .unwrap();
    assert!(err.0.contains("Dragon"));
}

#[test]
fn move_order_brings_unit_to_destination() {
    let mut b = Battle::new(&data(), &setup(2), BattleConfig::default()).unwrap();
    let dest = Vec2::new(-60.0, -80.0);
    b.step(&[Command::Move {
        unit: UnitId(0),
        dest,
        facing: Some(-Vec2::NORTH),
        width: None,
        run: false,
    }]);
    for _ in 0..20 * 40 {
        b.step(&[]);
    }
    let u = &b.units()[0];
    // Фронт смотрит на юг, значит строй стоит севернее точки (позади первой шеренги).
    let expected_center = dest + Vec2::NORTH * (u.depth() * 0.5 - u.stats.spacing_back * 0.5);
    assert!(
        u.center.distance(expected_center) < 2.0,
        "центр {:?}, ждали {:?}",
        u.center,
        expected_center
    );
    assert_eq!(u.state, UnitState::Ready);
}

#[test]
fn soldiers_do_not_overlap_much() {
    let mut b = Battle::new(&data(), &setup(3), BattleConfig::default()).unwrap();
    // Сгоняем два отряда в одну точку и проверяем, что расталкивание работает.
    let p = Vec2::new(0.0, -40.0);
    b.step(&[
        Command::Move {
            unit: UnitId(0),
            dest: p,
            facing: None,
            width: None,
            run: true,
        },
        Command::Move {
            unit: UnitId(1),
            dest: p,
            facing: None,
            width: None,
            run: true,
        },
    ]);
    for _ in 0..20 * 20 {
        b.step(&[]);
    }
    let pts: Vec<Vec2> = b
        .soldiers()
        .filter(|(s, _)| s.side == 0)
        .map(|(_, b)| b.pos)
        .collect();
    let mut close = 0;
    for i in 0..pts.len() {
        for j in i + 1..pts.len() {
            if pts[i].distance(pts[j]) < 0.4 {
                close += 1;
            }
        }
    }
    assert!(
        close < pts.len() / 10,
        "слишком много слипшихся пар: {close}"
    );
}

#[test]
fn battle_ends_with_a_winner() {
    let mut b = Battle::new(&data(), &setup(4), BattleConfig::default()).unwrap();
    for _ in 0..20 * 60 * 6 {
        let cmds = charge_all(&b);
        b.step(&cmds);
        if b.outcome().is_some() {
            break;
        }
    }
    assert!(b.outcome().is_some(), "битва не закончилась за 6 минут");
    assert!(
        b.killed[0] + b.killed[1] > 20,
        "почти никто не погиб: {:?}",
        b.killed
    );
}

#[test]
fn same_seed_same_battle() {
    let run = |seed| {
        let mut b = Battle::new(&data(), &setup(seed), BattleConfig::default()).unwrap();
        let mut checkpoints = Vec::new();
        for t in 0..20 * 60 {
            let cmds = charge_all(&b);
            b.step(&cmds);
            if t % 20 == 0 {
                checkpoints.push(b.state_hash());
            }
        }
        checkpoints
    };
    assert_eq!(run(7), run(7));
    assert_ne!(run(7), run(8));
}

#[test]
#[ignore]
fn trace() {
    let mut b = Battle::new(&data(), &setup(4), BattleConfig::default()).unwrap();
    for t in 0..20 * 60 * 6 {
        let cmds = charge_all(&b);
        b.step(&cmds);
        if t % 100 == 0 {
            let s: Vec<String> = b
                .units()
                .iter()
                .map(|u| format!("{}:{:?} n={} m={:.0}", u.id.0, u.state, u.alive, u.morale))
                .collect();
            println!(
                "t={:.0}s {} proj={}",
                b.time_seconds(),
                s.join(" | "),
                b.projectiles().count()
            );
        }
        if b.outcome().is_some() {
            println!(
                "{:?} at {}s killed {:?} fled {:?}",
                b.outcome(),
                b.time_seconds(),
                b.killed,
                b.fled
            );
            break;
        }
    }
}
