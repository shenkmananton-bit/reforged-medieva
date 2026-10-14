//! Кто чем владеет: таблица «регион → фракция» и цвета фракций.
//!
//! Это и есть хак H-04. Цвет региона на карте не хранится в текстуре:
//! шейдер берёт номер региона из текстуры номеров, по нему — номер
//! фракции-владельца из маленькой таблицы, по нему — цвет фракции.
//! Регион сменил владельца — в таблице поменялось одно число, а вся
//! карта перекрашивается в том же единственном вызове рисования.
//!
//! Правила кампании (кто кого завоевал) живут в `rm_campaign` (поток D8).
//! Здесь — только начальная раскладка для показа и то, как её нарисовать.

use std::collections::VecDeque;

use rm_core::Rng;

use crate::map::{CampaignMap, RegionId, SEA};

/// Номер фракции в списке [`Political::factions`].
pub type FactionIndex = u8;

/// Что рендеру нужно знать о фракции.
#[derive(Debug, Clone, PartialEq)]
pub struct FactionStyle {
    /// Внутреннее имя из `descr_sm_factions.txt`.
    pub name: String,
    /// Основной цвет, sRGB 0..255.
    pub colour: [u8; 3],
}

/// Владельцы регионов.
#[derive(Debug, Clone, PartialEq)]
pub struct Political {
    pub factions: Vec<FactionStyle>,
    /// `owner[id]` — фракция, владеющая регионом `id`. Запись моря не используется.
    pub owner: Vec<FactionIndex>,
    /// Мятежники (`slave` в оригинале), если они есть в списке фракций.
    pub rebels: Option<FactionIndex>,
    /// Счётчик изменений: рендер по нему понимает, что таблицу пора обновить.
    pub revision: u64,
}

/// Имя мятежников в файлах Medieval II.
pub const REBELS: &str = "slave";

impl Political {
    /// Начальная раскладка: у каждой фракции столица в своём углу карты,
    /// фракции по очереди забирают соседние регионы, пока не наберут
    /// примерно поровну. Остаток достаётся мятежникам.
    pub fn generate(map: &CampaignMap, factions: Vec<FactionStyle>, seed: u64) -> Self {
        let rebels = factions
            .iter()
            .position(|f| f.name == REBELS)
            .map(|i| i as FactionIndex);
        let playable: Vec<FactionIndex> = (0..factions.len() as FactionIndex)
            .filter(|&i| Some(i) != rebels)
            .collect();
        let regions = map.land_regions();
        let count = map.regions.len();
        let unowned = FactionIndex::MAX;
        let mut owner = vec![unowned; count];
        owner[SEA as usize] = rebels.unwrap_or(0);

        if !playable.is_empty() && !regions.is_empty() {
            let mut rng = Rng::new(seed ^ 0x0b0e_c7ed);
            let pos = |id: RegionId| {
                let s = map.regions[id as usize].settlement;
                (i64::from(s.0), i64::from(s.1))
            };
            let dist = |a: RegionId, b: RegionId| {
                let (pa, pb) = (pos(a), pos(b));
                (pa.0 - pb.0).pow(2) + (pa.1 - pb.1).pow(2)
            };
            // Столицы как можно дальше друг от друга.
            let mut capitals = vec![regions[rng.below(regions.len() as u32) as usize].id];
            while capitals.len() < playable.len().min(regions.len()) {
                let next = regions
                    .iter()
                    .map(|r| r.id)
                    .filter(|id| !capitals.contains(id))
                    .max_by_key(|&id| capitals.iter().map(|&c| dist(c, id)).min())
                    .unwrap_or(capitals[0]);
                capitals.push(next);
            }
            let share = if rebels.is_some() { 0.75 } else { 1.0 };
            let quota = ((regions.len() as f32 * share) / capitals.len() as f32).ceil() as usize;
            let mut fronts: Vec<VecDeque<RegionId>> = Vec::new();
            let mut held = vec![0usize; capitals.len()];
            for (k, &c) in capitals.iter().enumerate() {
                owner[c as usize] = playable[k];
                held[k] = 1;
                fronts.push(VecDeque::from([c]));
            }
            // По кругу: каждая фракция берёт одного соседа за раз.
            loop {
                let mut grew = false;
                for k in 0..capitals.len() {
                    if held[k] >= quota {
                        continue;
                    }
                    while let Some(&from) = fronts[k].front() {
                        let free = map.regions[from as usize]
                            .neighbours
                            .iter()
                            .copied()
                            .find(|&nb| owner[nb as usize] == unowned);
                        match free {
                            Some(nb) => {
                                owner[nb as usize] = playable[k];
                                held[k] += 1;
                                fronts[k].push_back(nb);
                                grew = true;
                                break;
                            }
                            None => {
                                fronts[k].pop_front();
                            }
                        }
                    }
                }
                if !grew {
                    break;
                }
            }
            // Ничьи регионы: мятежникам, а если их нет — ближайшей столице.
            for r in regions {
                if owner[r.id as usize] == unowned {
                    owner[r.id as usize] = rebels.unwrap_or_else(|| {
                        let k = (0..capitals.len())
                            .min_by_key(|&k| dist(capitals[k], r.id))
                            .unwrap_or(0);
                        playable[k]
                    });
                }
            }
        } else {
            for r in regions {
                owner[r.id as usize] = rebels.unwrap_or(0);
            }
        }

        Self {
            factions,
            owner,
            rebels,
            revision: 0,
        }
    }

    pub fn owner_of(&self, region: RegionId) -> FactionIndex {
        self.owner[region as usize]
    }

    /// Передать регион другой фракции. Это одна запись в таблицу — вся
    /// перекраска карты случится сама на следующем кадре (H-04).
    pub fn set_owner(&mut self, region: RegionId, faction: FactionIndex) {
        if region != SEA && (faction as usize) < self.factions.len() {
            self.owner[region as usize] = faction;
            self.revision += 1;
        }
    }

    /// Сколько регионов у фракции.
    pub fn count(&self, faction: FactionIndex) -> usize {
        self.owner[1..].iter().filter(|&&o| o == faction).count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::MapConfig;

    fn factions() -> Vec<FactionStyle> {
        ["northmark", "southreach", "steppe_horde", REBELS]
            .iter()
            .enumerate()
            .map(|(i, n)| FactionStyle {
                name: n.to_string(),
                colour: [i as u8 * 60, 100, 200],
            })
            .collect()
    }

    fn map() -> CampaignMap {
        CampaignMap::generate(&MapConfig {
            size: 160,
            regions: 24,
            ..MapConfig::default()
        })
    }

    #[test]
    fn every_region_gets_an_owner() {
        let m = map();
        let p = Political::generate(&m, factions(), 1);
        assert_eq!(p.rebels, Some(3));
        for r in m.land_regions() {
            assert!((p.owner_of(r.id) as usize) < p.factions.len());
        }
        // У каждой настоящей фракции есть хотя бы столица.
        for f in 0..3 {
            assert!(p.count(f) >= 1, "у фракции {f} нет земель");
        }
    }

    #[test]
    fn without_rebels_everything_is_owned_by_playable() {
        let m = map();
        let mut f = factions();
        f.pop();
        let p = Political::generate(&m, f, 1);
        assert_eq!(p.rebels, None);
        let total: usize = (0..3).map(|i| p.count(i)).sum();
        assert_eq!(total, m.land_regions().len());
    }

    #[test]
    fn set_owner_is_a_single_write() {
        let m = map();
        let mut p = Political::generate(&m, factions(), 1);
        let before = p.clone();
        let r = m.land_regions()[0].id;
        p.set_owner(r, 2);
        assert_eq!(p.owner_of(r), 2);
        assert_eq!(p.revision, 1);
        let changed = p
            .owner
            .iter()
            .zip(&before.owner)
            .filter(|(a, b)| a != b)
            .count();
        assert!(changed <= 1);
        // Море и несуществующая фракция игнорируются.
        p.set_owner(SEA, 1);
        p.set_owner(r, 99);
        assert_eq!(p.owner_of(r), 2);
    }
}
