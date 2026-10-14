//! Проверка папки данных для моддеров.
//!
//! Запуск: `cargo run -p rm_data --bin rm_modcheck -- data`
//!
//! Печатает все ошибки в виде `файл:строка: причина` и предупреждает
//! о полях, которые игра пока не понимает.

use std::path::PathBuf;
use std::process::ExitCode;

use rm_data::{FACTIONS_FILE, GameData, UNITS_FILE};

fn main() -> ExitCode {
    let dir = PathBuf::from(
        std::env::args()
            .nth(1)
            .unwrap_or_else(|| "data".to_string()),
    );
    match GameData::load_dir(&dir) {
        Ok(data) => {
            for u in &data.units {
                for x in &u.extra {
                    println!(
                        "{}:{}: предупреждение: поле `{}` пока не используется игрой",
                        dir.join(UNITS_FILE).display(),
                        x.line,
                        x.key
                    );
                }
            }
            for f in &data.factions {
                for x in &f.extra {
                    println!(
                        "{}:{}: предупреждение: поле `{}` пока не используется игрой",
                        dir.join(FACTIONS_FILE).display(),
                        x.line,
                        x.key
                    );
                }
            }
            // Файлы кампании проверяем, только если в папке есть кампания.
            if dir.join(rm_data::strat::STRAT_FILE).exists() {
                match rm_data::CampaignData::load_dir(&dir) {
                    Ok(c) => println!(
                        "Кампания в порядке: регионов {}, фракций на карте {}.",
                        c.regions.len(),
                        c.strat.factions.len()
                    ),
                    Err(errors) => {
                        eprintln!("{errors}");
                        eprintln!("Найдено ошибок в файлах кампании: {}.", errors.len());
                        return ExitCode::FAILURE;
                    }
                }
            }
            println!(
                "Всё в порядке: юнитов {}, фракций {}.",
                data.units.len(),
                data.factions.len()
            );
            ExitCode::SUCCESS
        }
        Err(errors) => {
            eprintln!("{errors}");
            eprintln!("Найдено ошибок: {}.", errors.len());
            ExitCode::FAILURE
        }
    }
}
