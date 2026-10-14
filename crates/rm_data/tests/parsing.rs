//! Тесты разбора: правильные файлы и понятные ошибки.

use std::path::{Path, PathBuf};

use rm_data::{GameData, UnitCategory, UnitClass, parse_factions, parse_units};

fn data_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data")
}

const UNIT: &str = "\
type             Test Spearmen
dictionary       Test_Spearmen ; комментарий
category         infantry
class            spearmen
soldier          spearman, 60, 0, 1
officer          officer_a
officer          officer_b
formation        1, 1.2, 2, 2.4, 4, square, schiltrom
stat_health      1, 0
stat_pri         5, 3, no, 0, 0, melee, simple, piercing, spear, 25, 1
stat_pri_armour  4, 4, 4, metal
stat_mental      6, disciplined, trained, lock_morale
stat_cost        1, 380, 140, 70, 60, 380, 4, 380
ownership        alpha, beta
my_mod_field     42
";

const FACTION: &str = "\
faction           alpha
culture           northern_european
religion          catholic
primary_colour    red 1, green 2, blue 3
secondary_colour  red 250, green 0, blue 0
can_sap           yes
";

fn units_err(text: &str) -> Vec<String> {
    parse_units(text, Path::new("u.txt"))
        .unwrap_err()
        .iter()
        .map(|e| e.to_string())
        .collect()
}

fn factions_err(text: &str) -> Vec<String> {
    parse_factions(text, Path::new("f.txt"))
        .unwrap_err()
        .iter()
        .map(|e| e.to_string())
        .collect()
}

#[test]
fn sample_data_folder_loads() {
    let data =
        GameData::load_dir(&data_dir()).expect("примерные данные должны загружаться без ошибок");
    assert!(data.units.len() >= 4);
    assert!(data.factions.len() >= 3);
    let archers = data.unit("Steppe Horse Archers").unwrap();
    assert_eq!(archers.category, UnitCategory::Cavalry);
    assert_eq!(archers.mount.as_deref(), Some("steppe pony"));
    assert_eq!(archers.primary_weapon.missile.as_deref(), Some("arrow"));
}

#[test]
fn parses_full_unit() {
    let units = parse_units(UNIT, Path::new("u.txt")).unwrap();
    let u = &units[0];
    assert_eq!(u.name, "Test Spearmen");
    assert_eq!(u.dictionary, "Test_Spearmen");
    assert_eq!(u.class, UnitClass::Spearmen);
    assert_eq!(u.soldier.count, 60);
    assert_eq!(u.officers, vec!["officer_a", "officer_b"]);
    let f = u.formation.as_ref().unwrap();
    assert_eq!(f.ranks, 4);
    assert_eq!(f.formations, vec!["square", "schiltrom"]);
    assert_eq!(u.primary_weapon.attack, 5);
    assert_eq!(u.primary_weapon.missile, None);
    assert!(u.secondary_weapon.is_none());
    assert_eq!(u.mental.flags, vec!["lock_morale"]);
    assert_eq!(u.cost.price, 380);
    assert_eq!(u.ownership, vec!["alpha", "beta"]);
    assert_eq!(u.line, 1);
    assert_eq!(u.extra.len(), 1);
    assert_eq!(u.extra[0].key, "my_mod_field");
    assert_eq!(u.extra[0].line, 15);
}

#[test]
fn parses_faction_with_defaults() {
    let f = &parse_factions(FACTION, Path::new("f.txt")).unwrap()[0];
    assert_eq!(f.primary_colour, rm_data::Rgb { r: 1, g: 2, b: 3 });
    assert!(f.can_sap);
    assert!(f.custom_battle_availability);
    assert!(!f.has_family_tree);
    assert_eq!(f.symbol, None);
}

#[test]
fn bad_number_points_to_line() {
    let text = UNIT.replace(
        "soldier          spearman, 60,",
        "soldier          spearman, sixty,",
    );
    let errs = units_err(&text);
    assert_eq!(errs.len(), 1, "{errs:?}");
    assert!(errs[0].starts_with("u.txt:5: "), "{}", errs[0]);
    assert!(errs[0].contains("sixty"), "{}", errs[0]);
}

#[test]
fn missing_required_field_points_to_record_start() {
    let text = UNIT.replace("stat_cost        1, 380, 140, 70, 60, 380, 4, 380\n", "");
    let errs = units_err(&text);
    assert_eq!(errs.len(), 1, "{errs:?}");
    assert!(errs[0].starts_with("u.txt:1: "), "{}", errs[0]);
    assert!(errs[0].contains("stat_cost"), "{}", errs[0]);
}

#[test]
fn collects_all_errors_at_once() {
    let text = UNIT
        .replace("category         infantry", "category         dragons")
        .replace("4, 4, 4, metal", "4, 4");
    let errs = units_err(&text);
    assert_eq!(errs.len(), 2, "{errs:?}");
    assert!(
        errs[0].contains("dragons") && errs[0].contains("infantry, cavalry"),
        "{}",
        errs[0]
    );
    assert!(errs[1].starts_with("u.txt:11: "), "{}", errs[1]);
}

#[test]
fn duplicate_field_and_duplicate_unit() {
    let text = UNIT.replace(
        "class            spearmen\n",
        "class            spearmen\nclass            heavy\n",
    );
    let errs = units_err(&text);
    assert!(
        errs[0].contains("повторяется") && errs[0].starts_with("u.txt:5: "),
        "{errs:?}"
    );

    let errs = units_err(&format!("{UNIT}\n{UNIT}"));
    assert_eq!(errs.len(), 1, "{errs:?}");
    assert!(errs[0].contains("уже объявлен на строке 1"), "{}", errs[0]);
}

#[test]
fn line_before_first_record() {
    let errs = units_err(&format!("dictionary oops\n{UNIT}"));
    assert!(
        errs[0].starts_with("u.txt:1: ") && errs[0].contains("до первой записи"),
        "{errs:?}"
    );
}

#[test]
fn bad_colour() {
    let errs =
        factions_err(&FACTION.replace("red 250, green 0, blue 0", "red 300, blue 0, green 0"));
    assert_eq!(errs.len(), 3, "{errs:?}");
    assert!(errs.iter().all(|e| e.starts_with("f.txt:5: ")), "{errs:?}");
}

#[test]
fn bad_bool() {
    let errs = factions_err(&FACTION.replace("can_sap           yes", "can_sap           maybe"));
    assert!(
        errs[0].contains("`yes` или `no`") && errs[0].contains("maybe"),
        "{errs:?}"
    );
}

#[test]
fn unknown_owner_is_reported() {
    let data = GameData {
        units: parse_units(UNIT, Path::new("u.txt")).unwrap(),
        factions: parse_factions(FACTION, Path::new("f.txt")).unwrap(),
    };
    let errs = data.check_references(Path::new("u.txt"));
    assert_eq!(errs.len(), 1);
    assert!(errs[0].to_string().contains("`beta`"), "{}", errs[0]);
}

#[test]
fn missing_file_is_readable() {
    let errs = GameData::load_dir(Path::new("/nonexistent/dir")).unwrap_err();
    assert_eq!(errs.len(), 2);
    assert!(errs.to_string().contains("файл не найден"));
}
