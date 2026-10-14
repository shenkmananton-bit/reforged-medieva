//! Запекание: всё освещение и окраска карты считаются один раз, заранее.
//!
//! Хак H-02. В игре видеокарта ничего не вычисляет про свет и тени —
//! она только читает готовые картинки:
//! * **albedo** — цвет земли без света (песок, трава, лес, скалы, снег, вода);
//! * **normal map** — наклон поверхности в каждой точке, чтобы мелкие
//!   складки рельефа были видны, хотя сетка треугольников грубая;
//! * **lightmap** — тень от гор (R) и затенение впадин, ambient occlusion (G);
//! * **поле границ** — расстояние до ближайшей границы региона (R)
//!   и номер региона по ту сторону (G), из него шейдер рисует границы.
//!
//! Сейчас запекание идёт при запуске (меньше секунды). Когда появится
//! конвейер ассетов (`rm_assets`, поток C2), эти картинки будут
//! запекаться офлайн и грузиться с диска. Подробно: `docs/campaign-map/baking.md`.

use crate::grid::{Grid, par_rows};
use crate::map::{CampaignMap, SEA, smoothstep};
use crate::noise;

/// Направление на солнце (к солнцу): с юго-запада, невысоко — длинные тени.
/// Ось x — восток, y — вверх, z — юг.
pub const SUN: [f32; 3] = [-0.55, 0.62, 0.56];

/// На сколько точек сетки поле границ помнит расстояние. Дальше — «далеко».
pub const BORDER_RANGE: f32 = 8.0;

/// Картинка с цепочкой уменьшенных копий (mip-уровней).
/// `levels[0]` — полный размер, каждый следующий вдвое меньше.
#[derive(Debug, Clone, PartialEq)]
pub struct Texture {
    pub size: u32,
    /// Байт на точку: 1, 2 или 4.
    pub channels: u32,
    pub levels: Vec<Vec<u8>>,
}

impl Texture {
    fn new(size: u32, channels: u32, base: Vec<u8>, mips: bool) -> Self {
        let mut levels = vec![base];
        if mips {
            let mut s = size;
            while s > 1 {
                let next = downsample(levels.last().unwrap(), s, channels);
                s /= 2;
                levels.push(next);
            }
        }
        Self {
            size,
            channels,
            levels,
        }
    }

    pub fn level_size(&self, level: usize) -> u32 {
        (self.size >> level).max(1)
    }

    pub fn byte_len(&self) -> usize {
        self.levels.iter().map(Vec::len).sum()
    }
}

/// Уменьшение вдвое: каждая точка — среднее из четырёх.
fn downsample(src: &[u8], size: u32, channels: u32) -> Vec<u8> {
    let half = (size / 2).max(1);
    let c = channels as usize;
    let mut out = vec![0u8; (half * half) as usize * c];
    for y in 0..half {
        for x in 0..half {
            for k in 0..c {
                let at = |dx: u32, dy: u32| {
                    let sx = (x * 2 + dx).min(size - 1);
                    let sy = (y * 2 + dy).min(size - 1);
                    u32::from(src[(sy * size + sx) as usize * c + k])
                };
                let sum = at(0, 0) + at(1, 0) + at(0, 1) + at(1, 1);
                out[(y * half + x) as usize * c + k] = ((sum + 2) / 4) as u8;
            }
        }
    }
    out
}

/// Все запечённые картинки карты.
#[derive(Debug, Clone)]
pub struct BakedMap {
    pub albedo: Texture,
    pub normal: Texture,
    pub light: Texture,
    pub border: Texture,
    /// Номера регионов как есть (без уменьшенных копий: номера нельзя усреднять).
    pub region: Texture,
}

impl BakedMap {
    pub fn bake(map: &CampaignMap) -> Self {
        let normals = normals(map);
        let light = lightmap(map);
        let albedo = albedo(map, &normals);
        let border = border_field(&map.region);
        let n = map.config.size;

        let mut normal_bytes = Vec::with_capacity(normals.len() * 4);
        for v in &normals {
            normal_bytes.extend(v.iter().map(|&c| ((c * 0.5 + 0.5) * 255.0).round() as u8));
            normal_bytes.push(255);
        }
        Self {
            albedo: Texture::new(n, 4, albedo, true),
            normal: Texture::new(n, 4, normal_bytes, true),
            light: Texture::new(n, 2, light, true),
            border: Texture::new(n, 2, border, false),
            region: Texture::new(n, 1, map.region.data.clone(), false),
        }
    }
}

/// Нормали по показываемым (вытянутым) высотам: горы на картинке
/// должны освещаться так же круто, как они выглядят.
fn normals(map: &CampaignMap) -> Vec<[f32; 3]> {
    let h = &map.display;
    let n = h.size;
    let inv2c = 0.5 / map.config.cell;
    let mut out = vec![[0.0f32; 3]; (n * n) as usize];
    par_rows(n, &mut out, |y, row| {
        let y = i64::from(y);
        for (x, v) in row.iter_mut().enumerate() {
            let x = x as i64;
            let dx = (h.get_clamped(x + 1, y) - h.get_clamped(x - 1, y)) * inv2c;
            let dz = (h.get_clamped(x, y + 1) - h.get_clamped(x, y - 1)) * inv2c;
            *v = normalize([-dx, 1.0, -dz]);
        }
    });
    out
}

/// Lightmap: R — тень от рельефа (луч к солнцу), G — затенение впадин.
fn lightmap(map: &CampaignMap) -> Vec<u8> {
    let h = &map.display;
    let n = h.size;
    let cell = map.config.cell;
    let horiz = (SUN[0] * SUN[0] + SUN[2] * SUN[2]).sqrt();
    let (dx, dz) = (SUN[0] / horiz, SUN[2] / horiz);
    let rise = SUN[1] / horiz * cell;
    let top = h.data.iter().copied().fold(0.0f32, f32::max);
    let blurred = box_blur(h, 6);

    let mut out = vec![[0u8; 2]; (n * n) as usize];
    par_rows(n, &mut out, |y, row| {
        for (x, px) in row.iter_mut().enumerate() {
            let h0 = h.get(x as u32, y) + 0.05;
            let mut lit = 1.0f32;
            let mut k = 1.0f32;
            // Идём от точки к солнцу, пока луч не поднимется выше всех гор.
            while h0 + rise * k <= top && k < 256.0 {
                let terrain = h.bilinear(x as f32 + dx * k, y as f32 + dz * k);
                let ray = h0 + rise * k;
                if terrain > ray {
                    // Мягкая тень: чем глубже луч под горой, тем темнее.
                    let occlusion = ((terrain - ray) / (1.0 + 0.12 * k)).min(1.0);
                    lit = lit.min(1.0 - occlusion);
                    if lit <= 0.0 {
                        break;
                    }
                }
                k += 1.0;
            }
            let i = (y * n) as usize + x;
            let ao = (1.0 - (blurred[i] - h.data[i]).max(0.0) * 0.05).clamp(0.6, 1.0);
            *px = [(lit * 255.0).round() as u8, (ao * 255.0).round() as u8];
        }
    });
    out.into_iter().flatten().collect()
}

/// Размытие квадратом (2r+1)×(2r+1), отдельно по строкам и по столбцам.
fn box_blur(h: &Grid<f32>, r: i64) -> Vec<f32> {
    let n = h.size;
    let w = (2 * r + 1) as f32;
    let mut rows = vec![0.0f32; h.data.len()];
    par_rows(n, &mut rows, |y, row| {
        for (x, v) in row.iter_mut().enumerate() {
            let x = x as i64;
            *v = (-r..=r)
                .map(|d| h.get_clamped(x + d, i64::from(y)))
                .sum::<f32>()
                / w;
        }
    });
    let tmp = Grid {
        size: n,
        data: rows,
    };
    let mut out = vec![0.0f32; h.data.len()];
    par_rows(n, &mut out, |y, row| {
        for (x, v) in row.iter_mut().enumerate() {
            let y = i64::from(y);
            *v = (-r..=r)
                .map(|d| tmp.get_clamped(x as i64, y + d))
                .sum::<f32>()
                / w;
        }
    });
    out
}

/// Цвет земли без освещения. Альфа: 255 — суша, 0 — вода.
fn albedo(map: &CampaignMap, normals: &[[f32; 3]]) -> Vec<u8> {
    let c = &map.config;
    let n = c.size;
    let seed = c.seed as u32 ^ 0x51;
    let inv = 1.0 / n as f32;
    let snow_line = c.mountain_start * c.max_height
        + (c.max_height * (1.0 - c.mountain_start)) * 0.55 * c.mountain_scale;

    const SAND: [f32; 3] = [196.0, 180.0, 130.0];
    const DRY: [f32; 3] = [132.0, 138.0, 72.0];
    const LUSH: [f32; 3] = [72.0, 112.0, 46.0];
    const FOREST: [f32; 3] = [38.0, 70.0, 36.0];
    const ROCK: [f32; 3] = [112.0, 102.0, 92.0];
    const SNOW: [f32; 3] = [236.0, 238.0, 244.0];
    const SHALLOW: [f32; 3] = [64.0, 132.0, 140.0];
    const DEEP: [f32; 3] = [18.0, 42.0, 78.0];

    let mut out = vec![[0u8; 4]; (n * n) as usize];
    par_rows(n, &mut out, |y, row| {
        for (x, px) in row.iter_mut().enumerate() {
            let i = (y * n) as usize + x;
            let (u, v) = (x as f32 * inv, y as f32 * inv);
            let h = map.height.data[i];
            let fine = noise::value(seed, u * 300.0, v * 300.0) * 0.12 + 0.94;
            if h <= 0.0 {
                let depth = (-h / c.sea_depth).clamp(0.0, 1.0).sqrt();
                let col = mix(SHALLOW, DEEP, depth);
                *px = rgba(col, 0.98 + fine * 0.02, 0);
                continue;
            }
            let shown = map.display.data[i];
            let slope = 1.0 - normals[i][1];
            let moisture = noise::fbm(seed ^ 0x3, u * 5.0, v * 5.0, 4);
            let mut col = mix(DRY, LUSH, smoothstep(0.35, 0.65, moisture));
            // Песок у берега.
            col = mix(SAND, col, smoothstep(0.25, 0.9, h));
            // Леса пятнами на равнинах и предгорьях.
            let woods = noise::fbm(seed ^ 0x9, u * 22.0, v * 22.0, 4) + moisture * 0.25;
            let forest =
                smoothstep(0.66, 0.74, woods) * (1.0 - smoothstep(0.2, 0.5, map.mountain.data[i]));
            col = mix(col, FOREST, forest * smoothstep(0.6, 1.5, h));
            // Скалы на крутых склонах и в горах.
            let rock =
                smoothstep(0.12, 0.35, slope).max(smoothstep(0.3, 0.6, map.mountain.data[i]));
            col = mix(col, ROCK, rock);
            // Снег на вершинах, но не на отвесных стенах.
            let snow = smoothstep(snow_line, snow_line + 4.0, shown)
                * (1.0 - smoothstep(0.35, 0.6, slope));
            col = mix(col, SNOW, snow);
            *px = rgba(col, fine, 255);
        }
    });
    out.into_iter().flatten().collect()
}

/// Поле границ. Для каждой точки суши: расстояние (в точках сетки) до
/// края её региона и номер региона за этим краем. Расстояние считается
/// только внутри своего региона — двумя проходами «сверху вниз» и
/// «снизу вверх» (chamfer distance transform), это быстро и без рекурсии.
/// R = расстояние / [`BORDER_RANGE`] · 255, G = номер соседа.
pub fn border_field(region: &Grid<u8>) -> Vec<u8> {
    let n = region.size as i64;
    let len = region.data.len();
    let mut dist = vec![f32::MAX; len];
    let mut other = vec![SEA; len];
    let id = |x: i64, y: i64| region.get(x as u32, y as u32);

    // Точки у самого края региона: расстояние полклетки до линии границы.
    for y in 0..n {
        for x in 0..n {
            let me = id(x, y);
            if me == SEA {
                continue;
            }
            let mut found: Option<u8> = None;
            for (dx, dy) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
                let (nx, ny) = (x + dx, y + dy);
                if nx < 0 || ny < 0 || nx >= n || ny >= n {
                    continue;
                }
                let nb = id(nx, ny);
                // Соседний регион суши важнее моря: морской берег — не граница фракций.
                if nb != me && (found.is_none() || found == Some(SEA)) {
                    found = Some(nb);
                }
            }
            if let Some(nb) = found {
                let i = (y * n + x) as usize;
                dist[i] = 0.5;
                other[i] = nb;
            }
        }
    }

    const DIAG: f32 = std::f32::consts::SQRT_2;
    let mut relax = |x: i64, y: i64, dx: i64, dy: i64, w: f32| {
        let (nx, ny) = (x + dx, y + dy);
        if nx < 0 || ny < 0 || nx >= n || ny >= n {
            return;
        }
        let (i, j) = ((y * n + x) as usize, (ny * n + nx) as usize);
        if region.data[i] == region.data[j] && dist[j] + w < dist[i] {
            dist[i] = dist[j] + w;
            other[i] = other[j];
        }
    };
    for y in 0..n {
        for x in 0..n {
            relax(x, y, -1, 0, 1.0);
            relax(x, y, 0, -1, 1.0);
            relax(x, y, -1, -1, DIAG);
            relax(x, y, 1, -1, DIAG);
        }
    }
    for y in (0..n).rev() {
        for x in (0..n).rev() {
            relax(x, y, 1, 0, 1.0);
            relax(x, y, 0, 1, 1.0);
            relax(x, y, 1, 1, DIAG);
            relax(x, y, -1, 1, DIAG);
        }
    }

    let mut out = Vec::with_capacity(len * 2);
    for i in 0..len {
        let d = (dist[i] / BORDER_RANGE).min(1.0);
        out.push((d * 255.0).round() as u8);
        out.push(other[i]);
    }
    out
}

fn normalize(v: [f32; 3]) -> [f32; 3] {
    let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt().max(1e-12);
    [v[0] / l, v[1] / l, v[2] / l]
}

fn mix(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
    ]
}

fn rgba(c: [f32; 3], k: f32, a: u8) -> [u8; 4] {
    let q = |v: f32| (v * k).round().clamp(0.0, 255.0) as u8;
    [q(c[0]), q(c[1]), q(c[2]), a]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::MapConfig;

    fn baked() -> (CampaignMap, BakedMap) {
        let m = CampaignMap::generate(&MapConfig {
            size: 128,
            regions: 16,
            ..MapConfig::default()
        });
        let b = BakedMap::bake(&m);
        (m, b)
    }

    #[test]
    fn textures_have_full_mip_chains() {
        let (_, b) = baked();
        assert_eq!(b.albedo.levels.len(), 8); // 128 → 1
        assert_eq!(b.albedo.levels[0].len(), 128 * 128 * 4);
        assert_eq!(b.light.levels[3].len(), 16 * 16 * 2);
        assert_eq!(b.albedo.levels.last().unwrap().len(), 4);
        assert_eq!(b.border.levels.len(), 1);
        assert_eq!(b.region.levels[0].len(), 128 * 128);
    }

    #[test]
    fn water_alpha_matches_sea() {
        let (m, b) = baked();
        for (i, &h) in m.height.data.iter().enumerate() {
            assert_eq!(b.albedo.levels[0][i * 4 + 3] == 255, h > 0.0);
        }
    }

    #[test]
    fn flat_ground_faces_up() {
        let mut m = CampaignMap::generate(&MapConfig {
            size: 32,
            regions: 2,
            ..MapConfig::default()
        });
        m.display.data.fill(3.0);
        let n = normals(&m);
        assert!(n.iter().all(|v| (v[1] - 1.0).abs() < 1e-6));
    }

    #[test]
    fn a_wall_casts_a_shadow_away_from_the_sun() {
        // Ровная земля и одна высокая стена посередине: точки за стеной
        // (с противоположной солнцу стороны) в тени, перед стеной — на свету.
        let mut m = CampaignMap::generate(&MapConfig {
            size: 64,
            regions: 2,
            ..MapConfig::default()
        });
        m.display.data.fill(1.0);
        for y in 0..64 {
            m.display.set(32, y, 30.0);
        }
        let light = lightmap(&m);
        let lit = |x: u32| light[((20 * 64 + x) * 2) as usize];
        // Солнце на западе (x < 0), значит тень — к востоку от стены.
        assert!(lit(35) < 60, "за стеной должно быть темно: {}", lit(35));
        assert!(lit(28) > 200, "перед стеной светло: {}", lit(28));
    }

    #[test]
    fn border_field_knows_distance_and_neighbour() {
        // Две полосы: регион 1 слева, регион 2 справа.
        let mut g = Grid::new(16, 1u8);
        for y in 0..16 {
            for x in 8..16 {
                g.set(x, y, 2);
            }
        }
        let f = border_field(&g);
        let at = |x: u32, y: u32| {
            let i = ((y * 16 + x) * 2) as usize;
            (f[i] as f32 / 255.0 * BORDER_RANGE, f[i + 1])
        };
        let (d, o) = at(7, 5);
        assert!((d - 0.5).abs() < 0.05 && o == 2, "{d} {o}");
        let (d, o) = at(9, 5);
        assert!((d - 1.5).abs() < 0.05 && o == 1, "{d} {o}");
        let (d, _) = at(3, 5);
        assert!((d - 4.5).abs() < 0.05, "{d}");
    }

    #[test]
    fn baking_is_deterministic() {
        let (_, a) = baked();
        let (_, b) = baked();
        assert_eq!(a.albedo, b.albedo);
        assert_eq!(a.light, b.light);
        assert_eq!(a.border, b.border);
    }
}
