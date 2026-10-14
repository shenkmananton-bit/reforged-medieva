//! Запекание normal map и lightmap из карты высот (хак H-02) и
//! преувеличение гор (хак H-05).
//!
//! Карта кампании в Medieval II не освещается в реальном времени: свет,
//! тени гор и рельеф посчитаны заранее и лежат в текстурах. Здесь это
//! делается из карты высот: нормаль — по разнице соседних высот, тень —
//! «лучом» к солнцу по сетке: если по пути есть точка выше луча, точка
//! в тени.

use crate::image::Image;
use crate::noise::fbm;

/// Карта высот в метрах, строки сверху вниз (как у `map_heights.tga`).
#[derive(Debug, Clone, PartialEq)]
pub struct Heightmap {
    pub width: u32,
    pub height: u32,
    pub data: Vec<f32>,
}

impl Heightmap {
    pub fn get(&self, x: i32, y: i32) -> f32 {
        let x = x.clamp(0, self.width as i32 - 1) as usize;
        let y = y.clamp(0, self.height as i32 - 1) as usize;
        self.data[y * self.width as usize + x]
    }

    /// Карта высот из серой картинки (например, `map_heights.tga`):
    /// яркость 0 → `min_m`, 255 → `max_m`. Берётся красный канал.
    pub fn from_image(img: &Image, min_m: f32, max_m: f32) -> Self {
        Self {
            width: img.width,
            height: img.height,
            data: img
                .pixels
                .iter()
                .map(|p| min_m + (max_m - min_m) * p[0] as f32 / 255.0)
                .collect(),
        }
    }

    /// Холмы и горы из шума, для примеров и тестов.
    pub fn procedural(width: u32, height: u32, max_m: f32, seed: u32) -> Self {
        let mut data = Vec::with_capacity((width * height) as usize);
        for y in 0..height {
            for x in 0..width {
                let n = fbm(
                    x as f32 / width as f32,
                    y as f32 / height as f32,
                    3,
                    5,
                    seed,
                );
                data.push((n - 0.3).max(0.0) / 0.7 * max_m);
            }
        }
        Self {
            width,
            height,
            data,
        }
    }

    /// Хак H-05: всё, что выше `threshold_m`, растягивается по высоте в `factor` раз.
    /// Горы становятся заметнее сверху и меньше мерцают, равнины не меняются.
    pub fn exaggerate_mountains(&mut self, threshold_m: f32, factor: f32) {
        for h in &mut self.data {
            if *h > threshold_m {
                *h = threshold_m + (*h - threshold_m) * factor;
            }
        }
    }

    fn normal(&self, x: i32, y: i32, cell: f32) -> [f32; 3] {
        let dx = (self.get(x + 1, y) - self.get(x - 1, y)) / (2.0 * cell);
        let dz = (self.get(x, y + 1) - self.get(x, y - 1)) / (2.0 * cell);
        let n = [-dx, 1.0, -dz];
        let len = rm_core::dmath::sqrt(n[0] * n[0] + 1.0 + n[2] * n[2]);
        [n[0] / len, n[1] / len, n[2] / len]
    }
}

/// Normal map: нормаль `(x, y, z)` упакована в RGB как `n * 0.5 + 0.5`,
/// ось y (вверх) — в зелёном. `cell_m` — размер клетки карты в метрах.
pub fn normal_map(hm: &Heightmap, cell_m: f32) -> Image {
    Image::from_fn(hm.width, hm.height, |x, y| {
        let n = hm.normal(x as i32, y as i32, cell_m);
        let q = |c: f32| ((c * 0.5 + 0.5).clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
        [q(n[0]), q(n[1]), q(n[2]), 255]
    })
}

/// Lightmap: яркость = рассеянный свет + солнце по Ламберту, в тени — только рассеянный.
/// `sun` — направление на солнце `(x, y, z)` с y вверх, z — вниз по строкам карты.
pub fn lightmap(hm: &Heightmap, cell_m: f32, sun: [f32; 3]) -> Image {
    let len = rm_core::dmath::sqrt(sun[0] * sun[0] + sun[1] * sun[1] + sun[2] * sun[2]);
    let s = [sun[0] / len, sun[1] / len, sun[2] / len];
    let horiz = rm_core::dmath::sqrt(s[0] * s[0] + s[2] * s[2]).max(1e-6);
    // Шаг луча — одна клетка по горизонтали; подъём на шаг — в метрах.
    let (step_x, step_z) = (s[0] / horiz, s[2] / horiz);
    let rise = s[1] / horiz * cell_m;
    let max_h = hm.data.iter().copied().fold(0.0, f32::max);
    Image::from_fn(hm.width, hm.height, |x, y| {
        let n = hm.normal(x as i32, y as i32, cell_m);
        let lambert = (n[0] * s[0] + n[1] * s[1] + n[2] * s[2]).max(0.0);
        let start = hm.get(x as i32, y as i32);
        let mut lit = true;
        let (mut px, mut pz, mut ray) = (x as f32, y as f32, start);
        loop {
            px += step_x;
            pz += step_z;
            ray += rise;
            if ray > max_h
                || px < 0.0
                || pz < 0.0
                || px >= hm.width as f32
                || pz >= hm.height as f32
            {
                break;
            }
            if hm.get(px as i32, pz as i32) > ray + 0.01 {
                lit = false;
                break;
            }
        }
        let v = 0.35 + if lit { lambert * 0.65 } else { 0.0 };
        let q = (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
        [q, q, q, 255]
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ridge() -> Heightmap {
        // Высокий хребет посередине, равнина по краям.
        let (w, h) = (32u32, 8u32);
        let data = (0..w * h)
            .map(|i| {
                if (14..18).contains(&(i % w)) {
                    50.0
                } else {
                    0.0
                }
            })
            .collect();
        Heightmap {
            width: w,
            height: h,
            data,
        }
    }

    #[test]
    fn flat_normal_points_up() {
        let flat = Heightmap {
            width: 4,
            height: 4,
            data: vec![3.0; 16],
        };
        let nm = normal_map(&flat, 10.0);
        assert!(nm.pixels.iter().all(|p| *p == [128, 255, 128, 255]));
    }

    #[test]
    fn mountain_casts_shadow_away_from_sun() {
        // Солнце низко на востоке (+x): тень ложится на запад от хребта.
        let lm = lightmap(&ridge(), 10.0, [1.0, 0.3, 0.0]);
        let west = lm.get(10, 4)[0];
        let east = lm.get(24, 4)[0];
        assert!(west < east, "запад {west}, восток {east}");
    }

    #[test]
    fn exaggeration_keeps_plains() {
        let mut hm = ridge();
        hm.exaggerate_mountains(10.0, 2.5);
        assert_eq!(hm.get(0, 0), 0.0);
        assert_eq!(hm.get(15, 0), 10.0 + 40.0 * 2.5);
    }

    #[test]
    fn grey_image_to_heights() {
        let img = Image::new(2, 2, [255, 255, 255, 255]);
        let hm = Heightmap::from_image(&img, -10.0, 90.0);
        assert_eq!(hm.get(1, 1), 90.0);
    }
}
