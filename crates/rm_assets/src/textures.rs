//! Процедурные текстуры материалов и альфа-маски грязи, снега и крови.
//!
//! Каждая текстура считается кодом из зерна (`seed`): одинаковое зерно —
//! одинаковая картинка до бита. Все текстуры бесшовные, их можно класть
//! плиткой (это нужно для почв на карте и в битве).
//!
//! Это заглушки «в духе» Medieval II, не настоящий арт: их задача — дать
//! рендеру правильные форматы и раскладку, чтобы позже художник заменил
//! картинки, не трогая код.

use crate::image::{Image, Rgba};
use crate::noise::{fbm, hash2, unit};

/// Материал: что за поверхность нарисована на куске атласа.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Material {
    // Почвы для карты кампании и поля битвы (texture splatting).
    Grass,
    Dirt,
    Sand,
    Rock,
    Snow,
    Mud,
    // Постройки и стены.
    Cobble,
    Plaster,
    Thatch,
    RoofTile,
    Wood,
    StoneBlock,
    // Солдаты и кони.
    Cloth,
    Leather,
    Steel,
    Skin,
    HorseHide,
    // Растительность.
    Bark,
    Foliage,
}

impl Material {
    pub const ALL: [Material; 19] = [
        Material::Grass,
        Material::Dirt,
        Material::Sand,
        Material::Rock,
        Material::Snow,
        Material::Mud,
        Material::Cobble,
        Material::Plaster,
        Material::Thatch,
        Material::RoofTile,
        Material::Wood,
        Material::StoneBlock,
        Material::Cloth,
        Material::Leather,
        Material::Steel,
        Material::Skin,
        Material::HorseHide,
        Material::Bark,
        Material::Foliage,
    ];

    pub const COUNT: usize = Self::ALL.len();

    /// Имя куска в атласе и в пакете: `mat/<имя>`.
    pub fn name(self) -> &'static str {
        match self {
            Material::Grass => "grass",
            Material::Dirt => "dirt",
            Material::Sand => "sand",
            Material::Rock => "rock",
            Material::Snow => "snow",
            Material::Mud => "mud",
            Material::Cobble => "cobble",
            Material::Plaster => "plaster",
            Material::Thatch => "thatch",
            Material::RoofTile => "roof_tile",
            Material::Wood => "wood",
            Material::StoneBlock => "stone_block",
            Material::Cloth => "cloth",
            Material::Leather => "leather",
            Material::Steel => "steel",
            Material::Skin => "skin",
            Material::HorseHide => "horse_hide",
            Material::Bark => "bark",
            Material::Foliage => "foliage",
        }
    }

    pub fn index(self) -> usize {
        self as usize
    }

    /// Почва ли это (такие текстуры кладутся плиткой на землю).
    pub fn is_terrain(self) -> bool {
        matches!(
            self,
            Material::Grass
                | Material::Dirt
                | Material::Sand
                | Material::Rock
                | Material::Snow
                | Material::Mud
        )
    }
}

fn mix(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
    ]
}

fn scale(c: [f32; 3], k: f32) -> [f32; 3] {
    [c[0] * k, c[1] * k, c[2] * k]
}

fn rgba(c: [f32; 3]) -> Rgba {
    let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
    [q(c[0]), q(c[1]), q(c[2]), 255]
}

/// Кладка из прямоугольных блоков: возвращает (номер блока, расстояние до шва 0..1).
/// Каждый второй ряд сдвинут на полблока, как в настоящей кладке.
fn bricks(u: f32, v: f32, cols: f32, rows: f32) -> (u32, f32) {
    let row = (v * rows).floor();
    let shift = if row as i32 % 2 == 0 { 0.0 } else { 0.5 };
    let x = u * cols + shift;
    let col = x.floor();
    let fx = x - col;
    let fy = v * rows - row;
    let edge = fx.min(1.0 - fx).min(fy.min(1.0 - fy) * cols / rows);
    let id = hash2((col as i32).rem_euclid(cols as i32), row as i32, 99);
    (id, edge)
}

/// Рисует квадратную текстуру материала со стороной `size` пикселей.
pub fn generate(m: Material, size: u32, seed: u32) -> Image {
    let s = seed.wrapping_add(m.index() as u32 * 7919);
    Image::from_fn(size, size, |x, y| {
        let u = (x as f32 + 0.5) / size as f32;
        let v = (y as f32 + 0.5) / size as f32;
        let n = fbm(u, v, 4, 4, s);
        let fine = fbm(u, v, 16, 2, s ^ 0xabcd);
        rgba(color(m, u, v, n, fine))
    })
}

fn color(m: Material, u: f32, v: f32, n: f32, fine: f32) -> [f32; 3] {
    match m {
        Material::Grass => mix([0.20, 0.34, 0.10], [0.42, 0.52, 0.18], n * 0.7 + fine * 0.3),
        Material::Dirt => mix([0.32, 0.23, 0.14], [0.50, 0.38, 0.24], n * 0.6 + fine * 0.4),
        Material::Sand => mix([0.70, 0.62, 0.42], [0.86, 0.78, 0.56], n * 0.5 + fine * 0.5),
        Material::Rock => {
            let ridge = 1.0 - (n * 2.0 - 1.0).abs();
            mix(
                [0.30, 0.29, 0.28],
                [0.58, 0.56, 0.52],
                ridge * 0.7 + fine * 0.3,
            )
        }
        Material::Snow => mix([0.80, 0.84, 0.90], [0.97, 0.98, 1.0], n * 0.5 + fine * 0.5),
        Material::Mud => mix([0.18, 0.13, 0.08], [0.32, 0.24, 0.15], n),
        Material::Cobble => {
            let (id, edge) = bricks(u, v, 6.0, 6.0);
            let base = mix([0.36, 0.34, 0.31], [0.55, 0.53, 0.49], unit(id));
            if edge < 0.08 {
                [0.16, 0.15, 0.13]
            } else {
                scale(base, 0.85 + fine * 0.3)
            }
        }
        Material::StoneBlock => {
            let (id, edge) = bricks(u, v, 3.0, 6.0);
            let base = mix([0.55, 0.52, 0.46], [0.70, 0.67, 0.60], unit(id));
            if edge < 0.05 {
                [0.35, 0.33, 0.29]
            } else {
                scale(base, 0.88 + fine * 0.24)
            }
        }
        Material::Plaster => mix([0.78, 0.72, 0.60], [0.90, 0.86, 0.76], n * 0.7 + fine * 0.3),
        Material::Thatch => {
            // Вертикальные пучки соломы.
            let streak = unit(hash2((u * 48.0) as i32, (v * 3.0) as i32, 5));
            mix(
                [0.48, 0.38, 0.18],
                [0.74, 0.62, 0.34],
                streak * 0.6 + n * 0.4,
            )
        }
        Material::RoofTile => {
            let (id, _) = bricks(u, v, 8.0, 8.0);
            let fy = (v * 8.0).fract();
            let base = mix([0.55, 0.22, 0.14], [0.70, 0.32, 0.20], unit(id));
            scale(base, 0.65 + fy * 0.45)
        }
        Material::Wood => {
            // Доски: вертикальные полосы с волокнами.
            let plank = (u * 4.0).floor();
            let fx = (u * 4.0).fract();
            let grain = fbm(u * 0.25, v, 2, 3, plank as u32 + 17);
            let base = mix([0.34, 0.22, 0.11], [0.52, 0.35, 0.19], grain);
            if !(0.04..=0.96).contains(&fx) {
                [0.18, 0.11, 0.06]
            } else {
                base
            }
        }
        Material::Cloth => {
            // Почти белая ткань с переплетением: шейдер умножит её на цвет фракции.
            let weave = if ((u * 64.0) as i32 + (v * 64.0) as i32) % 2 == 0 {
                1.0
            } else {
                0.93
            };
            scale([0.92, 0.92, 0.92], weave * (0.9 + n * 0.1))
        }
        Material::Leather => mix([0.30, 0.19, 0.10], [0.45, 0.30, 0.17], n),
        Material::Steel => {
            let brushed = fbm(u, v * 0.1, 8, 2, 3);
            mix(
                [0.48, 0.50, 0.54],
                [0.74, 0.76, 0.80],
                brushed * 0.7 + n * 0.3,
            )
        }
        Material::Skin => mix([0.76, 0.56, 0.42], [0.86, 0.66, 0.52], n),
        Material::HorseHide => mix([0.28, 0.18, 0.10], [0.42, 0.28, 0.17], n * 0.8 + fine * 0.2),
        Material::Bark => {
            let streak = fbm(u * 0.2, v, 4, 3, 77);
            mix([0.22, 0.15, 0.09], [0.38, 0.28, 0.18], streak)
        }
        Material::Foliage => mix([0.10, 0.24, 0.08], [0.24, 0.42, 0.14], n * 0.6 + fine * 0.4),
    }
}

/// Канал маски окружения (хак H-12).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MaskChannel {
    /// Красный: грязь, копится снизу.
    Dirt = 0,
    /// Зелёный: снег, копится сверху.
    Snow = 1,
    /// Синий: кровь, редкие пятна.
    Blood = 2,
}

/// Маска окружения для материала: R — грязь, G — снег, B — кровь, A = 255.
///
/// Хак H-12: испачканный или заснеженный солдат — та же модель с той же
/// текстурой. Шейдер смешивает диффузный цвет с цветом грязи/снега/крови
/// там, где значение маски меньше уровня загрязнения отряда (0..1). Растёт
/// уровень — пятно расползается. Без частиц и деформации меша.
///
/// Вертикальный градиент внутри куска атласа работает потому, что каждая
/// грань модели натягивается на кусок целиком: низ грани — низ куска.
pub fn generate_mask(m: Material, size: u32, seed: u32) -> Image {
    let s = seed.wrapping_add(m.index() as u32 * 104_729);
    Image::from_fn(size, size, |x, y| {
        let u = (x as f32 + 0.5) / size as f32;
        let v = (y as f32 + 0.5) / size as f32;
        let n = fbm(u, v, 4, 4, s);
        let spots = fbm(u, v, 8, 2, s ^ 0x5eed);
        // Маленькое значение = пятно появляется раньше всех.
        let dirt = (1.0 - v) * 0.6 + n * 0.4;
        let snow = v * 0.6 + n * 0.4;
        let blood = 1.0 - spots * spots * 1.2;
        let q = |t: f32| (t.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
        [q(dirt), q(snow), q(blood), 255]
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_unique() {
        let mut names: Vec<_> = Material::ALL.iter().map(|m| m.name()).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), Material::COUNT);
        for (i, m) in Material::ALL.iter().enumerate() {
            assert_eq!(m.index(), i);
        }
    }

    #[test]
    fn same_seed_same_texture() {
        for m in Material::ALL {
            assert_eq!(generate(m, 16, 3), generate(m, 16, 3));
        }
        assert_ne!(
            generate(Material::Grass, 16, 3),
            generate(Material::Grass, 16, 4)
        );
    }

    #[test]
    fn terrain_textures_tile_seamlessly() {
        // Соседние пиксели через шов отличаются не сильнее, чем внутри.
        for m in Material::ALL.into_iter().filter(|m| m.is_terrain()) {
            let t = generate(m, 64, 1);
            for y in 0..64 {
                let a = t.get(63, y);
                let b = t.get(0, y);
                let diff = (0..3)
                    .map(|c| (a[c] as i32 - b[c] as i32).abs())
                    .max()
                    .unwrap();
                assert!(diff < 40, "{} шов {diff}", m.name());
            }
        }
    }

    #[test]
    fn dirt_grows_from_bottom_and_snow_from_top() {
        let mask = generate_mask(Material::Cloth, 32, 0);
        let row_avg = |y: u32, c: usize| (0..32).map(|x| mask.get(x, y)[c] as u32).sum::<u32>();
        let ch = MaskChannel::Dirt as usize;
        assert!(row_avg(31, ch) < row_avg(0, ch));
        let ch = MaskChannel::Snow as usize;
        assert!(row_avg(0, ch) < row_avg(31, ch));
    }
}
