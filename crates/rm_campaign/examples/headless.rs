//! Кампания без окна: фракции ходят автопилотом, в консоль печатается
//! летопись. Запуск из корня репозитория:
//!
//! ```text
//! cargo run -p rm_campaign --example headless -- 20
//! ```
//!
//! Аргументы: число ходов (по умолчанию 10), seed (по умолчанию 1),
//! папка данных (по умолчанию `data`).

use std::path::PathBuf;
use std::process::ExitCode;

use rm_campaign::{Campaign, CampaignEvent, autoplay};
use rm_data::{CampaignData, Season};

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let turns: u32 = args.next().and_then(|s| s.parse().ok()).unwrap_or(10);
    let seed: u64 = args.next().and_then(|s| s.parse().ok()).unwrap_or(1);
    let dir = PathBuf::from(args.next().unwrap_or_else(|| "data".into()));

    let data = match CampaignData::load_dir(&dir) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    };
    let mut c = match Campaign::new(&data, seed) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    };

    let mut shown = None;
    while c.turn() < turns && !c.is_finished() {
        let turn = c.turn();
        if shown != Some(turn) {
            shown = Some(turn);
            let d = c.date();
            let season = match d.season {
                Season::Summer => "лето",
                Season::Winter => "зима",
            };
            println!("\n=== Ход {} — {season} {} года ===", turn + 1, d.year);
        }
        autoplay::play_turn(&mut c);
        for e in c.drain_events() {
            print_event(&c, &e);
        }
        c.end_turn();
    }
    for e in c.drain_events() {
        print_event(&c, &e);
    }

    println!("\n=== Итог ===");
    for f in c.factions() {
        let regions = c.settlements().iter().filter(|s| s.owner == f.id).count();
        let armies = c.armies().iter().filter(|a| a.owner == f.id).count();
        let status = if f.alive {
            ""
        } else {
            " (уничтожена)"
        };
        println!(
            "{:<14} казна {:>6}, регионов {regions}, армий {armies}{status}",
            f.name, f.treasury
        );
    }
    println!("Хеш состояния: {:016x}", c.state_hash());
    ExitCode::SUCCESS
}

fn print_event(c: &Campaign, e: &CampaignEvent) {
    let fname = |id: rm_campaign::FactionId| c.faction(id).name.as_str();
    let rname = |id: rm_campaign::RegionId| c.map().region(id).name.as_str();
    match e {
        CampaignEvent::TurnStarted { faction, .. } => println!("-- ходит {}", fname(*faction)),
        CampaignEvent::Income {
            report, treasury, ..
        } => println!(
            "   доход: налоги {} + фермы {} + торговля {} + кошелёк {} − армии {} = {:+}, в казне {treasury}",
            report.tax,
            report.farms,
            report.trade,
            report.kings_purse,
            report.upkeep,
            report.net()
        ),
        CampaignEvent::UnitRecruited { region, unit, .. } => println!(
            "   обучен отряд «{}» в {}",
            c.unit_type(*unit).name,
            rname(*region)
        ),
        CampaignEvent::ArmyMoved { from, to, .. } => {
            println!("   армия идёт: {} → {}", rname(*from), rname(*to))
        }
        CampaignEvent::Battle {
            region,
            attacker,
            defender,
            attacker_won,
            attacker_power,
            defender_power,
        } => println!(
            "   битва в {}: {} ({attacker_power}) против {} ({defender_power}) — {}",
            rname(*region),
            fname(*attacker),
            fname(*defender),
            if *attacker_won {
                "победа нападающих"
            } else {
                "нападающие разбиты"
            }
        ),
        CampaignEvent::ArmyDestroyed { faction, .. } => {
            println!("   армия {} уничтожена", fname(*faction))
        }
        CampaignEvent::SettlementCaptured { region, from, to } => println!(
            "   {} захватывает {} у {}",
            fname(*to),
            rname(*region),
            fname(*from)
        ),
        CampaignEvent::FactionDestroyed { faction } => {
            println!("   *** фракция {} уничтожена ***", fname(*faction))
        }
    }
}
