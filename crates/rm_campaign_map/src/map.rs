//! Процедурная карта-заглушка: рельеф, моря, горы и регионы.
//!
//! Настоящая карта Medieval II хранится в картинках (`map_heights.tga`,
//! `map_regions.tga` и т.д.). Оригинальные файлы класть в репозиторий
//! нельзя, поэтому, пока у нас нет своей нарисованной карты, мы строим
//! её из шума по зерну. Результат — те же самые данные, что дала бы
//! картинка: сетка высот и сетка номеров регионов. Рендеру всё равно,
//! откуда они пришли. Подробно: `docs/campaign-map/generation.md`.

use std::cmp::Reverse;
use std::collections::BinaryHeap;

use rm_core::Rng;

use crate::grid::{Grid, par_rows};
use crate::noise;

/// Номер региона. 0 — море, 1..=254 — суша.
pub type RegionId = u8;
pub const SEA: RegionId = 0;
/// Больше регионов не влезает в 8-битную текстуру номеров.
pub const MAX_REGIONS: u32 = 254;

/// Параметры генерации.
#[derive(Debug, Clone, PartialEq)]
pub struct MapConfig {
    pub seed: u64,
    /// Точек сетки по стороне (высоты и регионы одного размера).
    pub size: u32,
    /// Сколько регионов нарезать на суше.
    pub regions: u32,
    /// Какая доля карты — суша (0..1).
    pub land_fraction: f32,
    /// Размер точки сетки в единицах мира.
    pub cell: f32,
    /// Высота самой высокой горы «по правде», единицы мира.
    pub max_height: f32,
    /// Глубина самого глубокого моря, единицы мира.
    pub sea_depth: f32,
    /// Хак H-05: во сколько раз вытянуть горы по высоте при показе.
    pub mountain_scale: f32,
    /// С какой высоты (доля от `max_height`) начинаются «горы» для H-05.
    pub mountain_start: f32,
}

impl Default for MapConfig {
    fn default() -> Self {
        Self {
            seed: 1,
            size: 1024,
            regions: 48,
            land_fraction: 0.46,
            cell: 1.0,
            max_height: 24.0,
            sea_depth: 12.0,
            mountain_scale: 2.5,
            mountain_start: 0.28,
        }
    }
}

impl MapConfig {
    /// Сторона карты в единицах мира.
    pub fn world_size(&self) -> f32 {
        self.size as f32 * self.cell
    }
}

/// Один регион: провинция с поселением.
#[derive(Debug, Clone, PartialEq)]
pub struct Region {
    pub id: RegionId,
    pub name: String,
    /// Точка сетки, где стоит поселение.
    pub settlement: (u32, u32),
    /// Площадь в точках сетки.
    pub area: u32,
    /// Соседи по суше (общая граница).
    pub neighbours: Vec<RegionId>,
}

/// Карта кампании в «сыром» виде, до запекания текстур.
#[derive(Debug, Clone)]
pub struct CampaignMap {
    pub config: MapConfig,
    /// Настоящая высота: 0 — уровень моря, ниже нуля — дно.
    pub height: Grid<f32>,
    /// Высота для показа: горы вытянуты (H-05), вода плоская на нуле.
    pub display: Grid<f32>,
    /// «Гористость» 0..1: насколько точка — часть горного хребта.
    pub mountain: Grid<f32>,
    /// Номер региона в каждой точке.
    pub region: Grid<RegionId>,
    /// `regions[id]`; запись 0 — море, заглушка без поселения.
    pub regions: Vec<Region>,
}

impl CampaignMap {
    /// Строит карту по параметрам. Одинаковые параметры — одинаковая карта.
    pub fn generate(config: &MapConfig) -> Self {
        let config = MapConfig {
            regions: config.regions.clamp(1, MAX_REGIONS),
            ..config.clone()
        };
        let (mut height, mountain) = relief(&config);
        let mut islands = land_components(&height);
        // Крошечные островки — шум, а не земля: топим их. Островов не может
        // быть больше, чем регионов, — лишние (самые маленькие) тоже топим.
        let min_island = (config.size * config.size / 2500).max(8) as usize;
        islands.sort_by_key(|c| Reverse(c.len()));
        let keep = islands
            .iter()
            .take(config.regions as usize)
            .filter(|c| c.len() >= min_island)
            .count();
        for c in islands.drain(keep..) {
            for i in c {
                height.data[i as usize] = -0.5;
            }
        }

        let mut rng = Rng::new(config.seed ^ 0x5eed_4e91);
        let seeds = place_seeds(&config, &islands, &mountain, &mut rng);
        let region = grow_regions(&config, &height, &mountain, &seeds);
        let names = region_names(seeds.len(), &mut rng);
        let mut regions = vec![Region {
            id: SEA,
            name: "Море".into(),
            settlement: (0, 0),
            area: 0,
            neighbours: Vec::new(),
        }];
        for (i, (&s, name)) in seeds.iter().zip(names).enumerate() {
            regions.push(Region {
                id: (i + 1) as RegionId,
                name,
                settlement: (s % config.size, s / config.size),
                area: 0,
                neighbours: Vec::new(),
            });
        }
        fill_area_and_neighbours(&region, &mut regions);
        let display = display_heights(&config, &height);

        Self {
            config,
            height,
            display,
            mountain,
            region,
            regions,
        }
    }

    /// Регионы суши (без записи моря).
    pub fn land_regions(&self) -> &[Region] {
        &self.regions[1..]
    }

    /// Номер региона в точке мира `(x, z)` или море за краем карты.
    pub fn region_at_world(&self, x: f32, z: f32) -> RegionId {
        let (gx, gy) = (x / self.config.cell, z / self.config.cell);
        if gx < 0.0 || gy < 0.0 || gx >= self.config.size as f32 || gy >= self.config.size as f32 {
            return SEA;
        }
        self.region.get(gx as u32, gy as u32)
    }

    /// Высота показа в точке мира (для камеры и меток).
    pub fn display_height_at(&self, x: f32, z: f32) -> f32 {
        let c = self.config.cell;
        self.display.bilinear(x / c - 0.5, z / c - 0.5)
    }

    /// Точка мира в центре точки сетки.
    pub fn world_of(&self, (x, y): (u32, u32)) -> (f32, f32) {
        let c = self.config.cell;
        ((x as f32 + 0.5) * c, (y as f32 + 0.5) * c)
    }
}

/// Рельеф: настоящая высота и «гористость».
fn relief(config: &MapConfig) -> (Grid<f32>, Grid<f32>) {
    let n = config.size;
    let seed = config.seed as u32 ^ (config.seed >> 32) as u32;
    let inv = 1.0 / n as f32;

    // 1. «Материковость» e: шум с искажением координат минус спад к краю карты.
    let mut elevation = vec![0.0f32; (n * n) as usize];
    par_rows(n, &mut elevation, |y, row| {
        for (x, out) in row.iter_mut().enumerate() {
            let (u, v) = ((x as f32 + 0.5) * inv, (y as f32 + 0.5) * inv);
            let wu = u + (noise::fbm(seed ^ 0xa1, u * 2.0, v * 2.0, 3) - 0.5) * 0.35;
            let wv = v + (noise::fbm(seed ^ 0xb2, u * 2.0, v * 2.0, 3) - 0.5) * 0.35;
            let c = noise::fbm(seed, wu * 3.2, wv * 3.2, 7);
            let (du, dv) = ((u - 0.5) * 2.0, (v - 0.5) * 2.0);
            // Скруглённый квадрат: углы карты тоже уходят в море, без «креста» на воде.
            let edge = (du.powi(4) + dv.powi(4)).sqrt().sqrt();
            *out = c - smoothstep(0.6, 1.0, edge) * 0.45;
        }
    });

    // 2. Уровень моря подбираем так, чтобы суши было ровно `land_fraction`.
    let mut sorted = elevation.clone();
    sorted.sort_unstable_by(f32::total_cmp);
    let q = ((1.0 - config.land_fraction.clamp(0.05, 0.95)) * (sorted.len() - 1) as f32) as usize;
    let sea = sorted[q];
    let (lo, hi) = (sorted[0], sorted[sorted.len() - 1]);

    // 3. Горы: хребтовый шум там, где «маска гор» высокая, и не у самого берега.
    let mut mountain = Grid::new(n, 0.0f32);
    let mut height = Grid::new(n, 0.0f32);
    let mut pairs: Vec<(f32, f32)> = vec![(0.0, 0.0); (n * n) as usize];
    par_rows(n, &mut pairs, |y, row| {
        for (x, out) in row.iter_mut().enumerate() {
            let i = y as usize * n as usize + x;
            let (u, v) = ((x as f32 + 0.5) * inv, (y as f32 + 0.5) * inv);
            let e = elevation[i];
            if e <= sea {
                let depth = (sea - e) / (sea - lo).max(1e-6);
                *out = (-depth.sqrt() * config.sea_depth, 0.0);
                continue;
            }
            let t = (e - sea) / (hi - sea).max(1e-6);
            let hills = t.powf(1.2) * 0.3 + noise::fbm(seed ^ 0xc3, u * 24.0, v * 24.0, 3) * 0.04;
            let mask = smoothstep(0.46, 0.62, noise::fbm(seed ^ 0xd4, u * 2.6, v * 2.6, 4));
            let ridge = noise::ridged(seed ^ 0xe5, u * 7.0, v * 7.0, 5);
            let m = (ridge.powf(1.6) * mask * smoothstep(0.02, 0.12, t)).min(1.0);
            *out = ((hills + m * 0.75).min(1.0) * config.max_height, m);
        }
    });
    for (i, (h, m)) in pairs.into_iter().enumerate() {
        height.data[i] = h;
        mountain.data[i] = m;
    }
    (height, mountain)
}

/// Хак H-05: всё, что выше `mountain_start`, вытягивается по высоте
/// в `mountain_scale` раз. Вода показывается плоской на нуле.
fn display_heights(config: &MapConfig, height: &Grid<f32>) -> Grid<f32> {
    let start = config.mountain_start * config.max_height;
    Grid {
        size: height.size,
        data: height
            .data
            .iter()
            .map(|&h| display_height(h, start, config.mountain_scale))
            .collect(),
    }
}

pub fn display_height(h: f32, start: f32, scale: f32) -> f32 {
    if h <= 0.0 {
        0.0
    } else if h <= start {
        h
    } else {
        start + (h - start) * scale
    }
}

/// Связные куски суши (острова и материки), по соседству через сторону.
fn land_components(height: &Grid<f32>) -> Vec<Vec<u32>> {
    let n = height.size;
    let mut seen = vec![false; height.data.len()];
    let mut out = Vec::new();
    let mut stack = Vec::new();
    for start in 0..height.data.len() {
        if seen[start] || height.data[start] <= 0.0 {
            continue;
        }
        let mut comp = Vec::new();
        seen[start] = true;
        stack.push(start as u32);
        while let Some(i) = stack.pop() {
            comp.push(i);
            let (x, y) = (i % n, i / n);
            for (nx, ny) in neighbours4(x, y, n) {
                let j = (ny * n + nx) as usize;
                if !seen[j] && height.data[j] > 0.0 {
                    seen[j] = true;
                    stack.push(j as u32);
                }
            }
        }
        out.push(comp);
    }
    out
}

fn neighbours4(x: u32, y: u32, n: u32) -> impl Iterator<Item = (u32, u32)> {
    let mut v = [(0u32, 0u32); 4];
    let mut k = 0;
    if x > 0 {
        v[k] = (x - 1, y);
        k += 1;
    }
    if x + 1 < n {
        v[k] = (x + 1, y);
        k += 1;
    }
    if y > 0 {
        v[k] = (x, y - 1);
        k += 1;
    }
    if y + 1 < n {
        v[k] = (x, y + 1);
        k += 1;
    }
    v.into_iter().take(k)
}

/// Места поселений: каждому острову — число регионов по его площади
/// (минимум один), внутри острова — «лучший кандидат» Митчелла: из
/// нескольких случайных точек берём самую далёкую от уже выбранных.
/// Горные точки не берём: города стоят в долинах.
fn place_seeds(
    config: &MapConfig,
    islands: &[Vec<u32>],
    mountain: &Grid<f32>,
    rng: &mut Rng,
) -> Vec<u32> {
    let n = config.size;
    let total: usize = islands.iter().map(Vec::len).sum();
    let wanted = config.regions as usize;
    // Распределяем регионы по островам пропорционально площади.
    let mut quotas: Vec<usize> = islands
        .iter()
        .map(|c| ((c.len() * wanted) / total.max(1)).max(1))
        .collect();
    while quotas.iter().sum::<usize>() > wanted {
        let biggest = (0..quotas.len()).max_by_key(|&i| quotas[i]).unwrap_or(0);
        if quotas[biggest] <= 1 {
            break;
        }
        quotas[biggest] -= 1;
    }
    while quotas.iter().sum::<usize>() < wanted {
        let i = (0..islands.len())
            .max_by_key(|&i| islands[i].len() / (quotas[i] + 1))
            .unwrap_or(0);
        quotas[i] += 1;
    }

    let mut seeds: Vec<u32> = Vec::new();
    for (island, &quota) in islands.iter().zip(&quotas) {
        let flat: Vec<u32> = island
            .iter()
            .copied()
            .filter(|&i| mountain.data[i as usize] < 0.15)
            .collect();
        let pool = if flat.len() >= quota { &flat } else { island };
        let first = seeds.len();
        for _ in 0..quota {
            let mut best = (0u32, -1i64);
            for _ in 0..24 {
                let c = pool[rng.below(pool.len() as u32) as usize];
                let (cx, cy) = (i64::from(c % n), i64::from(c / n));
                let d = seeds[first..]
                    .iter()
                    .map(|&s| {
                        let (sx, sy) = (i64::from(s % n), i64::from(s / n));
                        (sx - cx).pow(2) + (sy - cy).pow(2)
                    })
                    .min()
                    .unwrap_or(i64::MAX);
                if d > best.1 {
                    best = (c, d);
                }
            }
            seeds.push(best.0);
        }
    }
    seeds
}

/// Регионы растут от поселений одновременно, как пятна краски (алгоритм
/// Дейкстры). Шаг через горы стоит дороже, поэтому хребты сами становятся
/// границами — как на настоящих картах. Немного шума делает границы
/// извилистыми. Через море регионы не растут.
fn grow_regions(
    config: &MapConfig,
    height: &Grid<f32>,
    mountain: &Grid<f32>,
    seeds: &[u32],
) -> Grid<RegionId> {
    let n = config.size;
    let seed = config.seed as u32 ^ 0x77;
    let mut region = Grid::new(n, SEA);
    let mut cost = vec![u32::MAX; region.data.len()];
    let mut heap = BinaryHeap::new();
    for (i, &s) in seeds.iter().enumerate() {
        cost[s as usize] = 0;
        region.data[s as usize] = (i + 1) as RegionId;
        heap.push(Reverse((0u32, s)));
    }
    let inv = 1.0 / n as f32;
    while let Some(Reverse((c, i))) = heap.pop() {
        if c > cost[i as usize] {
            continue;
        }
        let id = region.data[i as usize];
        let (x, y) = (i % n, i / n);
        for (nx, ny) in neighbours4(x, y, n) {
            let j = (ny * n + nx) as usize;
            if height.data[j] <= 0.0 {
                continue;
            }
            let wobble = noise::fbm(seed, nx as f32 * inv * 18.0, ny as f32 * inv * 18.0, 3);
            let step = 1000.0 * (1.0 + mountain.data[j] * 7.0 + wobble * 1.6);
            let nc = c.saturating_add(step as u32);
            if nc < cost[j] {
                cost[j] = nc;
                region.data[j] = id;
                heap.push(Reverse((nc, j as u32)));
            }
        }
    }
    region
}

fn fill_area_and_neighbours(region: &Grid<RegionId>, regions: &mut [Region]) {
    let n = region.size;
    let count = regions.len();
    let mut adjacent = vec![false; count * count];
    for y in 0..n {
        for x in 0..n {
            let a = region.get(x, y);
            regions[a as usize].area += 1;
            for (nx, ny) in [(x + 1, y), (x, y + 1)] {
                if nx < n && ny < n {
                    let b = region.get(nx, ny);
                    if a != b && a != SEA && b != SEA {
                        adjacent[a as usize * count + b as usize] = true;
                        adjacent[b as usize * count + a as usize] = true;
                    }
                }
            }
        }
    }
    for (a, r) in regions.iter_mut().enumerate() {
        r.neighbours = (0..count)
            .filter(|&b| adjacent[a * count + b])
            .map(|b| b as RegionId)
            .collect();
    }
}

/// Названия-заглушки из слогов: «Велиград», «Тормир»… Все разные.
fn region_names(count: usize, rng: &mut Rng) -> Vec<String> {
    const HEADS: &[&str] = &[
        "Бел", "Вел", "Вол", "Гор", "Дуб", "Зар", "Кам", "Лис", "Мир", "Нов", "Ост", "Рад", "Сла",
        "Тор", "Хол", "Яр", "Бор", "Вер", "Кор", "Лу", "Мор", "Сер", "Тал", "Ул",
    ];
    const MIDS: &[&str] = &["", "", "о", "е", "и", "ан", "ен"];
    const TAILS: &[&str] = &[
        "град", "мир", "ск", "ец", "поль", "дол", "ин", "ов", "бор", "ница", "вар", "гард",
    ];
    let mut names: Vec<String> = Vec::with_capacity(count);
    while names.len() < count {
        let pick =
            |rng: &mut Rng, list: &[&'static str]| list[rng.below(list.len() as u32) as usize];
        let name = format!(
            "{}{}{}",
            pick(rng, HEADS),
            pick(rng, MIDS),
            pick(rng, TAILS)
        );
        if !names.contains(&name) {
            names.push(name);
        } else if names.len() > 1500 {
            names.push(format!("{name}{}", names.len()));
        }
    }
    names
}

pub fn smoothstep(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[cfg(test)]
mod tests {
    use super::*;

    pub fn small() -> MapConfig {
        MapConfig {
            size: 160,
            regions: 20,
            ..MapConfig::default()
        }
    }

    #[test]
    fn same_seed_same_map() {
        let a = CampaignMap::generate(&small());
        let b = CampaignMap::generate(&small());
        assert_eq!(a.height, b.height);
        assert_eq!(a.region, b.region);
        let c = CampaignMap::generate(&MapConfig { seed: 2, ..small() });
        assert_ne!(a.region, c.region);
    }

    #[test]
    fn land_fraction_is_close_to_config() {
        let m = CampaignMap::generate(&small());
        let land = m.height.data.iter().filter(|&&h| h > 0.0).count() as f32;
        let frac = land / m.height.data.len() as f32;
        // Тонущие островки чуть уменьшают сушу.
        assert!((frac - 0.46).abs() < 0.06, "суша {frac}");
    }

    #[test]
    fn every_land_point_has_a_region_and_sea_has_none() {
        let m = CampaignMap::generate(&small());
        for (h, &r) in m.height.data.iter().zip(&m.region.data) {
            assert_eq!(*h > 0.0, r != SEA, "h={h} r={r}");
        }
        assert_eq!(m.regions.len(), 21);
        for r in m.land_regions() {
            assert!(r.area > 0, "{} пустой", r.name);
            assert_eq!(m.region.get(r.settlement.0, r.settlement.1), r.id);
        }
    }

    #[test]
    fn regions_are_connected() {
        // Каждый регион — одно связное пятно (выросло из одного поселения).
        let m = CampaignMap::generate(&small());
        let n = m.config.size;
        for r in m.land_regions() {
            let mut seen = vec![false; m.region.data.len()];
            let mut stack = vec![(r.settlement.0, r.settlement.1)];
            let mut count = 0;
            seen[m.region.index(r.settlement.0, r.settlement.1)] = true;
            while let Some((x, y)) = stack.pop() {
                count += 1;
                for (nx, ny) in neighbours4(x, y, n) {
                    let j = m.region.index(nx, ny);
                    if !seen[j] && m.region.data[j] == r.id {
                        seen[j] = true;
                        stack.push((nx, ny));
                    }
                }
            }
            assert_eq!(count, r.area, "регион {} разорван", r.name);
        }
    }

    #[test]
    fn neighbours_are_symmetric() {
        let m = CampaignMap::generate(&small());
        for r in m.land_regions() {
            for &nb in &r.neighbours {
                assert!(m.regions[nb as usize].neighbours.contains(&r.id));
            }
        }
        assert!(m.land_regions().iter().any(|r| !r.neighbours.is_empty()));
    }

    #[test]
    fn mountains_are_exaggerated_on_display() {
        let c = MapConfig::default();
        let start = c.mountain_start * c.max_height;
        assert_eq!(display_height(-3.0, start, 2.5), 0.0);
        assert_eq!(display_height(start * 0.5, start, 2.5), start * 0.5);
        let peak = c.max_height;
        let shown = display_height(peak, start, 2.5);
        assert!(
            shown > peak * 1.9,
            "вершина {shown} должна быть заметно выше {peak}"
        );
    }

    #[test]
    fn names_are_unique() {
        let m = CampaignMap::generate(&small());
        let mut names: Vec<_> = m.land_regions().iter().map(|r| r.name.clone()).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), m.land_regions().len());
    }
}
