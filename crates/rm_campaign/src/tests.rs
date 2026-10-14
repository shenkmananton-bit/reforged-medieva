//! Модульные тесты правил на маленькой карте из трёх регионов:
//!
//! ```text
//! Alpha — Beta — Gamma
//! ```
//!
//! `red` владеет Alpha, `blue` — Gamma, Beta у мятежников (`slave`).

use std::path::Path;

use rm_data::campaign::{parse_campaign_rules, parse_region_links};
use rm_data::region::parse_regions;
use rm_data::resource::parse_resources;
use rm_data::strat::parse_strat;
use rm_data::{CampaignData, GameData, parse_factions, parse_units};

use super::*;

const UNITS: &str = "
type             Militia
dictionary       Militia
category         infantry
class            light
soldier          militia, 50, 0, 1
stat_health      1, 0
stat_pri         3, 1, no, 0, 0, melee, simple, blunt, none, 25, 1
stat_pri_armour  1, 1, 0, flesh
stat_mental      3, low, untrained
stat_cost        2, 200, 50, 30, 30, 200, 6, 200
ownership        red, blue, slave

type             Riders
dictionary       Riders
category         cavalry
class            light
soldier          rider, 20, 0, 1.4
stat_health      1, 1
stat_pri         5, 4, no, 0, 0, melee, simple, piercing, none, 25, 1
stat_pri_armour  2, 2, 0, leather
stat_mental      5, normal, trained
stat_cost        1, 500, 100, 30, 30, 500, 4, 500
ownership        blue
";

const FACTIONS: &str = "
faction red
culture northern_european
religion catholic
primary_colour red 200, green 0, blue 0
secondary_colour red 0, green 0, blue 0

faction blue
culture northern_european
religion catholic
primary_colour red 0, green 0, blue 200
secondary_colour red 0, green 0, blue 0

faction slave
culture northern_european
religion catholic
primary_colour red 90, green 90, blue 90
secondary_colour red 0, green 0, blue 0
";

const REGIONS: &str = "
Alpha
    Alphaburg
    red
    Rebels
    255 0 0
    gold
    1
    2
    religions { catholic 100 }
Beta
    Betaburg
    slave
    Rebels
    0 255 0
    none
    1
    0
    religions { catholic 100 }
Gamma
    Gammaburg
    blue
    Rebels
    0 0 255
    none
    1
    1
    religions { catholic 100 }
";

const RESOURCES: &str = "
type gold
trade_value 100
";

const LINKS: &str = "
region Alpha
position 0, 0
neighbours Beta
region Beta
position 10, 0
neighbours Alpha, Gamma
region Gamma
position 20, 0
neighbours Beta
";

const RULES: &str = "
tax_per_100_population 2
farm_income_per_level 10
growth_base_permille 10
growth_per_farm_level_permille 5
army_moves_per_turn 1
cavalry_army_moves_per_turn 2
recruit_slots_per_turn 1
autoresolve_spread_percent 20
autoresolve_winner_loss_percent 50
settlement_level village, 1050
settlement_level town, 5000
";

const STRAT: &str = "
campaign test
playable
    red
    blue
end
start_date 1100 summer
end_date 1110 winter
timescale 1.50

faction red, balanced smith
denari 1000
denari_kings_purse 100
settlement
{
    level village
    region Alpha
    population 1000
}
character Rex, named character, leader, x 1, y 0
army
unit Militia exp 0 armour 0 weapon_lvl 0
unit Militia exp 0 armour 0 weapon_lvl 0

faction blue, balanced smith
denari 300
denari_kings_purse 0
settlement
{
    level town
    region Gamma
    population 2000
}
character Bleu, named character, leader, x 19, y 0
army
unit Riders exp 0 armour 0 weapon_lvl 0

faction slave, slave_faction
denari 0
settlement
{
    level village
    region Beta
    population 500
}
";

fn data_with(strat: &str) -> CampaignData {
    let p = Path::new("test");
    let data = CampaignData {
        game: GameData {
            units: parse_units(UNITS, p).unwrap(),
            factions: parse_factions(FACTIONS, p).unwrap(),
        },
        regions: parse_regions(REGIONS, p).unwrap(),
        resources: parse_resources(RESOURCES, p).unwrap(),
        region_links: parse_region_links(LINKS, p).unwrap(),
        rules: parse_campaign_rules(RULES, p).unwrap(),
        strat: parse_strat(strat, p).unwrap(),
    };
    assert_eq!(data.check_references(p), vec![]);
    data
}

fn campaign() -> Campaign {
    Campaign::new(&data_with(STRAT), 7).unwrap()
}

fn id(c: &Campaign, name: &str) -> FactionId {
    c.faction_id(name).unwrap()
}

fn region(c: &Campaign, name: &str) -> RegionId {
    c.map().find(name).unwrap()
}

fn army_of(c: &Campaign, faction: &str) -> ArmyView {
    let f = id(c, faction);
    c.armies().into_iter().find(|a| a.owner == f).unwrap()
}

#[test]
fn setup_places_settlements_and_armies() {
    let c = campaign();
    assert_eq!(c.factions().len(), 3);
    assert_eq!(c.settlements().len(), 3);
    let red = army_of(&c, "red");
    // Персонаж стоит в клетке (1, 0) — ближе всего к поселению Alpha.
    assert_eq!(red.region, region(&c, "Alpha"));
    assert_eq!(red.general.as_deref(), Some("Rex"));
    assert_eq!(red.units.len(), 2);
    assert_eq!(red.units[0].soldiers, 50);
    // Первым ходит red, и его ход уже начался.
    assert_eq!(c.current_faction(), id(&c, "red"));
    assert!(c.faction(id(&c, "red")).playable);
    assert!(!c.faction(id(&c, "slave")).playable);
}

#[test]
fn income_and_growth_follow_the_rules_file() {
    let c = campaign();
    let red = c.faction(id(&c, "red"));
    // Рост: 1000 × (10 + 2 × 5) / 1000 = +20, но предел деревни 1050.
    assert_eq!(
        c.settlement_in(region(&c, "Alpha")).unwrap().population,
        1020
    );
    // Налоги: 1020 × 2 / 100 = 20; фермы: 2 × 10 = 20; торговля: золото 100;
    // кошелёк 100; содержание: 2 × 50.
    let r = red.last_income;
    assert_eq!(
        (r.tax, r.farms, r.trade, r.kings_purse, r.upkeep),
        (20, 20, 100, 100, 100)
    );
    assert_eq!(r.net(), 140);
    assert_eq!(red.treasury, 1000 + 140);
}

#[test]
fn population_is_capped_by_level() {
    let mut c = campaign();
    let red = id(&c, "red");
    // Три полных круга: 1020 → 1040 → 1050 (предел) → 1050.
    while c.turn() < 3 || c.current_faction() != red {
        c.end_turn();
    }
    assert_eq!(
        c.settlement_in(region(&c, "Alpha")).unwrap().population,
        1050
    );
}

#[test]
fn turn_order_and_date() {
    let mut c = campaign();
    let order: Vec<_> = (0..4)
        .map(|_| {
            let f = c.current_faction();
            c.end_turn();
            (c.faction(f).name.clone(), c.turn())
        })
        .collect();
    assert_eq!(order[0].0, "red");
    assert_eq!(order[1].0, "blue");
    assert_eq!(order[2].0, "slave");
    assert_eq!(order[3].0, "red");
    // После мятежников начался ход 1.
    assert_eq!(order[2].1, 1);
    // timescale 1.50: ход 1 — 1101 зима, ход 2 — 1103 лето.
    assert_eq!(
        c.date(),
        Date {
            year: 1101,
            season: Season::Winter
        }
    );
    while c.turn() < 2 {
        c.end_turn();
    }
    assert_eq!(
        c.date(),
        Date {
            year: 1103,
            season: Season::Summer
        }
    );
}

#[test]
fn recruitment_costs_money_and_takes_turns() {
    let mut c = campaign();
    let alpha = region(&c, "Alpha");
    let militia = c.unit_type_id("Militia").unwrap();
    let riders = c.unit_type_id("Riders").unwrap();
    let before = c.faction(c.current_faction()).treasury;
    assert_eq!(
        c.apply(Command::Recruit {
            region: alpha,
            unit: riders
        }),
        Err(CommandError::UnitNotAvailable)
    );
    assert_eq!(
        c.apply(Command::Recruit {
            region: region(&c, "Gamma"),
            unit: militia
        }),
        Err(CommandError::NotYourSettlement)
    );
    c.apply(Command::Recruit {
        region: alpha,
        unit: militia,
    })
    .unwrap();
    assert_eq!(c.faction(c.current_faction()).treasury, before - 200);

    // recruit_turns = 2: отряд готов в начале второго следующего хода red.
    let red = id(&c, "red");
    let units = |c: &Campaign| army_of(c, "red").units.len();
    for expected in [2, 3] {
        c.end_turn();
        while c.current_faction() != red {
            c.end_turn();
        }
        assert_eq!(units(&c), expected);
    }
    assert!(c.settlement_in(alpha).unwrap().queue.is_empty());
}

#[test]
fn recruitment_needs_money() {
    let mut c = campaign();
    c.end_turn(); // ход blue: 300 + доход
    let blue = id(&c, "blue");
    let riders = c.unit_type_id("Riders").unwrap();
    let have = c.faction(blue).treasury;
    assert!(have < 500);
    assert_eq!(
        c.apply(Command::Recruit {
            region: region(&c, "Gamma"),
            unit: riders
        }),
        Err(CommandError::NotEnoughMoney { need: 500, have })
    );
}

#[test]
fn move_rules() {
    let mut c = campaign();
    let red = army_of(&c, "red").entity;
    let blue = army_of(&c, "blue").entity;
    let gamma = region(&c, "Gamma");
    let beta = region(&c, "Beta");
    assert_eq!(
        c.apply(Command::MoveArmy {
            army: red,
            to: gamma
        }),
        Err(CommandError::NotAdjacent)
    );
    assert_eq!(
        c.apply(Command::MoveArmy {
            army: blue,
            to: beta
        }),
        Err(CommandError::NotYourArmy)
    );
    // Beta без защитников: вход = захват.
    c.apply(Command::MoveArmy {
        army: red,
        to: beta,
    })
    .unwrap();
    assert_eq!(c.army(red).unwrap().region, beta);
    assert_eq!(c.settlement_in(beta).unwrap().owner, id(&c, "red"));
    assert_eq!(
        c.apply(Command::MoveArmy {
            army: red,
            to: region(&c, "Alpha")
        }),
        Err(CommandError::NoMovesLeft)
    );
    // У мятежников не осталось ни поселений, ни армий.
    assert!(!c.faction(id(&c, "slave")).alive);
    let events = c.drain_events();
    assert!(events.contains(&CampaignEvent::SettlementCaptured {
        region: beta,
        from: id(&c, "slave"),
        to: id(&c, "red"),
    }));
    assert!(events.contains(&CampaignEvent::FactionDestroyed {
        faction: id(&c, "slave")
    }));
}

#[test]
fn cavalry_moves_twice() {
    let mut c = campaign();
    c.end_turn();
    let blue = army_of(&c, "blue");
    assert_eq!(blue.moves, 2);
    assert_eq!(army_of(&c, "red").moves, 1);
}

#[test]
fn battle_destroys_loser_and_captures() {
    let mut c = campaign();
    let red = army_of(&c, "red").entity;
    c.end_turn(); // blue
    let blue_army = army_of(&c, "blue").entity;
    let beta = region(&c, "Beta");
    let alpha = region(&c, "Alpha");
    c.apply(Command::MoveArmy {
        army: blue_army,
        to: beta,
    })
    .unwrap();
    c.drain_events();
    // Конница blue: 20 × (5 + 2 + 2 + 0 + 5) = 280; red: 2 × 50 × (3 + 1 + 1 + 0 + 3) = 800.
    assert_eq!(c.army_power(&army_of(&c, "blue").units), 280);
    assert_eq!(c.army_power(&army_of(&c, "red").units), 800);
    c.apply(Command::MoveArmy {
        army: blue_army,
        to: alpha,
    })
    .unwrap();
    let events = c.drain_events();
    let battle = events
        .iter()
        .find_map(|e| match e {
            CampaignEvent::Battle { attacker_won, .. } => Some(*attacker_won),
            _ => None,
        })
        .expect("был бой");
    if battle {
        assert!(c.army(red).is_none());
        assert_eq!(c.settlement_in(alpha).unwrap().owner, id(&c, "blue"));
        assert!(c.army(blue_army).unwrap().units[0].soldiers < 20);
    } else {
        assert!(c.army(blue_army).is_none());
        assert_eq!(c.settlement_in(alpha).unwrap().owner, id(&c, "red"));
        assert!(c.army(red).unwrap().units.iter().any(|u| u.soldiers < 50));
    }
}

#[test]
fn same_seed_same_battle() {
    let run = |seed| {
        let mut c = Campaign::new(&data_with(STRAT), seed).unwrap();
        for _ in 0..40 {
            autoplay::play_turn(&mut c);
            c.end_turn();
        }
        c.state_hash()
    };
    assert_eq!(run(3), run(3));
}

#[test]
fn campaign_ends_when_one_contender_left() {
    let mut c = campaign();
    let mut guard = 0;
    while !c.is_finished() && guard < 200 {
        autoplay::play_turn(&mut c);
        c.end_turn();
        guard += 1;
    }
    assert!(c.is_finished());
    assert_eq!(
        c.apply(Command::Recruit {
            region: region(&c, "Alpha"),
            unit: UnitTypeId(0)
        }),
        Err(CommandError::CampaignOver)
    );
}
