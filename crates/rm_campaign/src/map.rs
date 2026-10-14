//! Граф регионов: кто с кем граничит и как дойти из одного региона в другой.
//!
//! Правилам кампании не нужна сама картинка карты — только регионы и их
//! соседство. Как карта выглядит, решает рендер (`rm_campaign_map`); он
//! читает эти же данные через [`CampaignMap`].

use std::collections::VecDeque;

use rm_data::{CampaignData, Rgb};

use crate::ids::RegionId;

/// Регион на карте кампании.
#[derive(Debug, Clone, PartialEq)]
pub struct RegionInfo {
    pub id: RegionId,
    pub name: String,
    /// Имя поселения-столицы.
    pub settlement_name: String,
    /// Цвет региона на карте регионов.
    pub colour: Rgb,
    /// Клетка поселения на карте кампании.
    pub x: i32,
    pub y: i32,
    /// Соседи в порядке из `descr_rm_regions.txt`.
    pub neighbours: Vec<RegionId>,
    /// Уровень плодородия.
    pub farming_level: u32,
    /// Сумма торговой ценности ресурсов региона, денариев за ход.
    pub trade_value: u32,
}

/// Все регионы и их связи.
#[derive(Debug, Clone, PartialEq)]
pub struct CampaignMap {
    regions: Vec<RegionInfo>,
}

impl CampaignMap {
    /// Строит граф из проверенных данных (ссылки уже проверены в `rm_data`).
    pub fn from_data(data: &CampaignData) -> Self {
        let id_of = |name: &str| {
            data.regions
                .iter()
                .position(|r| r.name == name)
                .map(|i| RegionId(i as u16))
        };
        let regions = data
            .regions
            .iter()
            .enumerate()
            .map(|(i, r)| {
                let link = data.region_link(&r.name);
                RegionInfo {
                    id: RegionId(i as u16),
                    name: r.name.clone(),
                    settlement_name: r.settlement.clone(),
                    colour: r.colour,
                    x: link.map_or(0, |l| l.x),
                    y: link.map_or(0, |l| l.y),
                    neighbours: link
                        .map(|l| l.neighbours.iter().filter_map(|n| id_of(n)).collect())
                        .unwrap_or_default(),
                    farming_level: r.farming_level,
                    trade_value: r
                        .resources
                        .iter()
                        .filter_map(|res| data.resource(res))
                        .map(|res| res.trade_value)
                        .sum(),
                }
            })
            .collect();
        Self { regions }
    }

    pub fn regions(&self) -> &[RegionInfo] {
        &self.regions
    }

    pub fn region(&self, id: RegionId) -> &RegionInfo {
        &self.regions[id.index()]
    }

    pub fn find(&self, name: &str) -> Option<RegionId> {
        self.regions.iter().find(|r| r.name == name).map(|r| r.id)
    }

    pub fn are_neighbours(&self, a: RegionId, b: RegionId) -> bool {
        self.region(a).neighbours.contains(&b)
    }

    /// Регион, чьё поселение ближе всего к клетке `(x, y)`. Так персонажи
    /// из `descr_strat.txt` (там у них координаты) попадают в регион.
    /// При равенстве побеждает регион, объявленный раньше.
    pub fn nearest(&self, x: i32, y: i32) -> Option<RegionId> {
        self.regions
            .iter()
            .min_by_key(|r| {
                let dx = i64::from(r.x - x);
                let dy = i64::from(r.y - y);
                dx * dx + dy * dy
            })
            .map(|r| r.id)
    }

    /// Кратчайший путь по числу переходов (поиск в ширину). Первый элемент —
    /// `from`, последний — `to`. Соседи перебираются в порядке из файла,
    /// поэтому путь всегда один и тот же.
    pub fn path(&self, from: RegionId, to: RegionId) -> Option<Vec<RegionId>> {
        self.path_to(from, |r| r == to)
    }

    /// Кратчайший путь до ближайшего региона, для которого `goal` истинно.
    pub fn path_to(
        &self,
        from: RegionId,
        mut goal: impl FnMut(RegionId) -> bool,
    ) -> Option<Vec<RegionId>> {
        let mut prev: Vec<Option<RegionId>> = vec![None; self.regions.len()];
        let mut seen = vec![false; self.regions.len()];
        let mut queue = VecDeque::from([from]);
        seen[from.index()] = true;
        while let Some(r) = queue.pop_front() {
            if goal(r) {
                let mut path = vec![r];
                let mut cur = r;
                while let Some(p) = prev[cur.index()] {
                    path.push(p);
                    cur = p;
                }
                path.reverse();
                return Some(path);
            }
            for &n in &self.region(r).neighbours {
                if !seen[n.index()] {
                    seen[n.index()] = true;
                    prev[n.index()] = Some(r);
                    queue.push_back(n);
                }
            }
        }
        None
    }
}
