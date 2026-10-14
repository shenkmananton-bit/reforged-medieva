//! Тесты разбора файлов кампании: регионы, ресурсы, расстановка,
//! правила и соседство регионов.

use std::path::{Path, PathBuf};

use rm_data::campaign::{parse_campaign_rules, parse_region_links};
use rm_data::region::parse_regions;
use rm_data::resource::parse_resources;
use rm_data::strat::parse_strat;
use rm_data::{CampaignData, DataErrors, Date, Season};

fn data_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data")
}

fn messages(e: DataErrors) -> Vec<String> {
    e.iter()
        .map(|e| format!("{}: {}", e.line, e.message))
        .collect()
}

#[test]
fn real_campaign_data_loads() {
    let data = CampaignData::load_dir(&data_dir()).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(data.regions.len(), 10);
    assert_eq!(data.region_links.len(), 10);
    assert_eq!(data.strat.factions.len(), 4);
    assert_eq!(data.strat.factions[0].name, "northmark");
    assert_eq!(data.rules.level("town"), Some(1));
}

const REGION: &str = "\
Ostmark                 ; имя
    Ostburg
    northmark
    Northern_Rebels
    40 70 160
    iron, furs
    5
    3
    religions { catholic 90 pagan 10 }
";

#[test]
fn region_is_parsed_by_position() {
    let r = &parse_regions(REGION, Path::new("r.txt")).unwrap()[0];
    assert_eq!(r.name, "Ostmark");
    assert_eq!(r.settlement, "Ostburg");
    assert_eq!(r.creator, "northmark");
    assert_eq!((r.colour.r, r.colour.g, r.colour.b), (40, 70, 160));
    assert_eq!(r.resources, ["iron", "furs"]);
    assert_eq!(r.triumph_value, 5);
    assert_eq!(r.farming_level, 3);
    assert_eq!(
        r.religions,
        [("catholic".to_string(), 90), ("pagan".to_string(), 10)]
    );
    assert_eq!(r.line, 1);
}

#[test]
fn region_errors_name_the_line() {
    let missing_line = REGION.replace("    5\n", "");
    let errs = messages(parse_regions(&missing_line, Path::new("r.txt")).unwrap_err());
    assert!(
        errs[0].starts_with("1: у региона должно быть ровно 9 строк"),
        "{errs:?}"
    );

    let bad_religion = REGION.replace("pagan 10", "pagan");
    let errs = messages(parse_regions(&bad_religion, Path::new("r.txt")).unwrap_err());
    assert!(errs[0].starts_with("9: "), "{errs:?}");

    let unfinished = REGION.replace("    religions { catholic 90 pagan 10 }\n", "");
    let errs = messages(parse_regions(&unfinished, Path::new("r.txt")).unwrap_err());
    assert!(errs[0].contains("регион не закончен"), "{errs:?}");
}

#[test]
fn resources_parse() {
    let r = parse_resources(
        "type iron\ntrade_value 40\nhas_mine\nmy_field 1\n",
        Path::new("res.txt"),
    )
    .unwrap();
    assert_eq!(r[0].name, "iron");
    assert_eq!(r[0].trade_value, 40);
    assert!(r[0].has_mine);
    assert_eq!(r[0].extra[0].key, "my_field");
}

const STRAT: &str = "\
campaign        test
playable
    alpha
end
start_date      1080 summer
end_date        1100 winter
timescale       0.5
resource        iron, 10, 20
faction         alpha, comfortable caution
denari          100
denari_kings_purse  50
settlement
{
    level           town
    region          Ostmark
    population      2400
    building
    {
        type core_building wooden_pallisade
    }
}
character   Aldric, named character, male, leader, age 41, x 20, y 60
traits      GoodCommander 1
army
unit        Shield Wall Spearmen     exp 1 armour 2 weapon_lvl 0
unit        Village Levy
core_attitudes  alpha, 600, beta
";

#[test]
fn strat_subset_parses() {
    let s = parse_strat(STRAT, Path::new("s.txt")).unwrap();
    assert_eq!(s.campaign, "test");
    assert_eq!(s.playable, ["alpha"]);
    assert_eq!(
        s.start_date,
        Date {
            year: 1080,
            season: Season::Summer
        }
    );
    assert_eq!(s.timescale_hundredths, 50);
    let f = &s.factions[0];
    assert_eq!(f.ai_personality.as_deref(), Some("comfortable caution"));
    assert_eq!((f.denari, f.kings_purse), (100, 50));
    assert_eq!(f.settlements[0].region, "Ostmark");
    assert_eq!(
        f.settlements[0].buildings,
        ["core_building wooden_pallisade"]
    );
    let c = &f.characters[0];
    assert_eq!(
        (c.name.as_str(), c.kind.as_str()),
        ("Aldric", "named character")
    );
    assert_eq!((c.x, c.y, c.age), (20, 60, Some(41)));
    assert_eq!(c.flags, ["male", "leader"]);
    assert_eq!(c.army.len(), 2);
    assert_eq!(c.army[0].name, "Shield Wall Spearmen");
    assert_eq!((c.army[0].exp, c.army[0].armour), (1, 2));
    assert_eq!(c.army[1].name, "Village Levy");
    // Незнакомые строки не теряются.
    assert_eq!(f.extra[0].key, "traits");
    let top: Vec<_> = s.extra.iter().map(|x| x.key.as_str()).collect();
    assert_eq!(top, ["resource", "core_attitudes"]);
}

#[test]
fn strat_errors() {
    let no_end = STRAT.replace("start_date      1080 summer\n", "");
    let errs = messages(parse_strat(&no_end, Path::new("s.txt")).unwrap_err());
    assert!(errs.iter().any(|e| e.contains("`start_date`")), "{errs:?}");

    let bad_season = STRAT.replace("1080 summer", "1080 spring");
    let errs = messages(parse_strat(&bad_season, Path::new("s.txt")).unwrap_err());
    assert!(errs[0].starts_with("5: время года"), "{errs:?}");

    let orphan_unit =
        "campaign t\nstart_date 1 summer\nend_date 2 summer\ntimescale 1\nfaction a\nunit Levy\n";
    let errs = messages(parse_strat(orphan_unit, Path::new("s.txt")).unwrap_err());
    assert!(errs[0].contains("после персонажа"), "{errs:?}");
}

#[test]
fn rules_require_every_number() {
    let errs = messages(
        parse_campaign_rules("tax_per_100_population 3\n", Path::new("c.txt")).unwrap_err(),
    );
    assert!(errs.iter().any(|e| e.contains("farm_income_per_level")));
    assert!(errs.iter().any(|e| e.contains("settlement_level")));
}

#[test]
fn reference_errors_are_reported() {
    let dir = data_dir();
    let mut data = CampaignData::load_dir(&dir).unwrap();
    // Несимметричное соседство.
    data.region_links = parse_region_links(
        &std::fs::read_to_string(dir.join(rm_data::campaign::REGION_LINKS_FILE))
            .unwrap()
            .replace("neighbours  Ostmark, Greywater", "neighbours  Greywater"),
        Path::new("links.txt"),
    )
    .unwrap();
    // Неизвестный юнит в армии.
    data.strat.factions[0].characters[0].army[0].name = "Dragon".into();
    let errs: Vec<String> = data
        .check_references(&dir)
        .into_iter()
        .map(|e| e.message)
        .collect();
    assert!(
        errs.iter()
            .any(|e| e.contains("соседство должно быть взаимным")),
        "{errs:?}"
    );
    assert!(errs.iter().any(|e| e.contains("юнит `Dragon`")), "{errs:?}");
}
