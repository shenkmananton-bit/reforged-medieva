//! Запекание импосторов: модель рисуется программно в картинку.
//!
//! Это маленький программный растеризатор (без GPU): ортогональная
//! проекция спереди (камера на +z смотрит на модель), z-буфер, текстура
//! из атласа материалов и простое освещение от фиксированного солнца.
//! Запекается один раз при сборке ассетов, в игре только читается.
//!
//! **Цвет фракции в импосторе.** Картинка одна на все фракции, поэтому
//! окраска закодирована в альфа-канале «полосами»:
//!
//! | альфа | значение |
//! |---|---|
//! | 0 | пусто (пиксель отбрасывается) |
//! | 85 | умножить на второй цвет фракции |
//! | 170 | умножить на основной цвет фракции |
//! | 255 | цвет как есть |
//!
//! Шейдер читает такой атлас без фильтрации (nearest) и декодирует
//! полосу через [`decode_alpha`]. Это тот же приём «смена данных вместо
//! смены текстуры», что и в H-11/H-16.

use crate::image::{Image, Rgba};
use crate::mesh::{Mesh, Tint};

/// Направление на солнце при запекании (нормированное).
const SUN: [f32; 3] = [0.371_390_7, 0.742_781_4, 0.557_086_05];

/// Альфа пикселя для окраски `tint`.
pub fn encode_alpha(tint: Tint) -> u8 {
    match tint {
        Tint::None => 255,
        Tint::Primary => 170,
        Tint::Secondary => 85,
    }
}

/// Обратное преобразование: `None` — пустой пиксель.
pub fn decode_alpha(a: u8) -> Option<Tint> {
    match a {
        0..=42 => None,
        43..=127 => Some(Tint::Secondary),
        128..=212 => Some(Tint::Primary),
        _ => Some(Tint::None),
    }
}

/// Запечённый импостор: картинка и её размер в метрах.
#[derive(Debug, Clone, PartialEq)]
pub struct BakedImpostor {
    pub image: Image,
    pub width: f32,
    pub height: f32,
}

/// Рисует `mesh` спереди. `textures` — атлас материалов (UV меша указывают в него),
/// `pixels_per_meter` — плотность, `max_px` — предел большей стороны картинки.
pub fn bake(mesh: &Mesh, textures: &Image, pixels_per_meter: f32, max_px: u32) -> BakedImpostor {
    let (lo, hi) = mesh.bounds();
    let width = (hi[0] - lo[0]).max(0.01);
    let height = hi[1].max(0.01);
    let mut ppm = pixels_per_meter;
    let longest = width.max(height);
    if longest * ppm > max_px as f32 {
        ppm = max_px as f32 / longest;
    }
    let w = ((width * ppm).ceil() as u32).max(1);
    let h = ((height * ppm).ceil() as u32).max(1);
    let mut image = Image::new(w, h, [0, 0, 0, 0]);
    let mut depth = vec![f32::MIN; (w * h) as usize];

    // Пиксельные координаты вершины: x вправо, y вниз.
    let to_px = |p: [f32; 3]| -> [f32; 2] {
        [
            (p[0] - lo[0]) / width * w as f32,
            (1.0 - p[1] / height) * h as f32,
        ]
    };

    for tri in mesh.indices.as_chunks::<3>().0 {
        let v = [
            &mesh.vertices[tri[0] as usize],
            &mesh.vertices[tri[1] as usize],
            &mesh.vertices[tri[2] as usize],
        ];
        let n = v[0].normal;
        // Грани, смотрящие от камеры, не видны.
        if n[2] < -1e-4 {
            continue;
        }
        let light = (n[0] * SUN[0] + n[1] * SUN[1] + n[2] * SUN[2]).max(0.0) * 0.65 + 0.4;
        let alpha = encode_alpha(Tint::from_f32(v[0].tint));
        let p = [to_px(v[0].pos), to_px(v[1].pos), to_px(v[2].pos)];
        let area = edge(p[0], p[1], p[2]);
        if area.abs() < 1e-9 {
            continue;
        }
        let min_x = p
            .iter()
            .map(|q| q[0])
            .fold(f32::MAX, f32::min)
            .floor()
            .max(0.0) as u32;
        let max_x = p
            .iter()
            .map(|q| q[0])
            .fold(f32::MIN, f32::max)
            .ceil()
            .min(w as f32) as u32;
        let min_y = p
            .iter()
            .map(|q| q[1])
            .fold(f32::MAX, f32::min)
            .floor()
            .max(0.0) as u32;
        let max_y = p
            .iter()
            .map(|q| q[1])
            .fold(f32::MIN, f32::max)
            .ceil()
            .min(h as f32) as u32;
        for y in min_y..max_y {
            for x in min_x..max_x {
                let c = [x as f32 + 0.5, y as f32 + 0.5];
                let b0 = edge(p[1], p[2], c) / area;
                let b1 = edge(p[2], p[0], c) / area;
                let b2 = edge(p[0], p[1], c) / area;
                if b0 < 0.0 || b1 < 0.0 || b2 < 0.0 {
                    continue;
                }
                let z = b0 * v[0].pos[2] + b1 * v[1].pos[2] + b2 * v[2].pos[2];
                let i = (y * w + x) as usize;
                if z <= depth[i] {
                    continue;
                }
                depth[i] = z;
                let u = b0 * v[0].uv[0] + b1 * v[1].uv[0] + b2 * v[2].uv[0];
                let t = b0 * v[0].uv[1] + b1 * v[1].uv[1] + b2 * v[2].uv[1];
                let tex = textures.sample(u, t);
                image.set(x, y, shade(tex, light, alpha));
            }
        }
    }
    BakedImpostor {
        image,
        width,
        height,
    }
}

/// Поворачивает меш на 90° вокруг вертикали, чтобы запечь его сбоку
/// (вид с +x). Нужно моделям, плоским спереди, — например, знамени:
/// его полотнище лежит в плоскости y–z и спереди видно только ребром.
pub fn side_view(mesh: &Mesh) -> Mesh {
    let rot = |p: [f32; 3]| [-p[2], p[1], p[0]];
    let mut m = mesh.clone();
    for v in &mut m.vertices {
        v.pos = rot(v.pos);
        v.normal = rot(v.normal);
    }
    m
}

fn edge(a: [f32; 2], b: [f32; 2], c: [f32; 2]) -> f32 {
    (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
}

fn shade(tex: Rgba, light: f32, alpha: u8) -> Rgba {
    let l = |c: u8| ((c as f32 * light).min(255.0)) as u8;
    [l(tex[0]), l(tex[1]), l(tex[2]), alpha]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mesh::MaterialUvs;
    use crate::models::ModelId;

    #[test]
    fn alpha_bands_round_trip() {
        for t in [Tint::None, Tint::Primary, Tint::Secondary] {
            assert_eq!(decode_alpha(encode_alpha(t)), Some(t));
        }
        assert_eq!(decode_alpha(0), None);
    }

    #[test]
    fn soldier_impostor_has_silhouette_and_team_colour() {
        let uvs = MaterialUvs::identity();
        let tex = Image::new(4, 4, [200, 200, 200, 255]);
        let mesh = ModelId::Spearman.build(&uvs, 0);
        let imp = bake(&mesh, &tex, 32.0, 128);
        assert!(imp.height > 1.7 && imp.height < 2.0);
        let filled = imp.image.pixels.iter().filter(|p| p[3] != 0).count();
        let total = imp.image.pixels.len();
        assert!(filled > total / 10 && filled < total, "{filled} из {total}");
        // Туловище в цвет фракции: центр картинки на высоте груди.
        let chest = imp
            .image
            .get(imp.image.width / 2, (imp.image.height as f32 * 0.38) as u32);
        assert_eq!(decode_alpha(chest[3]), Some(Tint::Primary));
    }

    #[test]
    fn banner_is_visible_from_the_side() {
        let uvs = MaterialUvs::identity();
        let tex = Image::new(1, 1, [255; 4]);
        let mesh = ModelId::Banner.build(&uvs, 0);
        let front = bake(&mesh, &tex, 16.0, 128);
        let side = bake(&side_view(&mesh), &tex, 16.0, 128);
        let filled = |i: &BakedImpostor| i.image.pixels.iter().filter(|p| p[3] != 0).count();
        assert!(filled(&side) > filled(&front) * 3);
    }

    #[test]
    fn respects_max_size() {
        let uvs = MaterialUvs::identity();
        let tex = Image::new(1, 1, [255; 4]);
        let imp = bake(&ModelId::Church.build(&uvs, 0), &tex, 64.0, 96);
        assert!(imp.image.width <= 96 && imp.image.height <= 96);
    }
}
