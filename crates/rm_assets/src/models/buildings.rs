//! Постройки поселений: дом, церковь, сторожевая башня, амбар.
//!
//! Размеры в метрах, модель стоит центром основания в начале координат,
//! фасад (дверь) смотрит на +z. Здания повторяются в городе сотни раз,
//! поэтому рендер мегаполиса рисует их через GPU instancing (хак H-13),
//! а этот модуль даёт для них LOD-цепочки.

use crate::mesh::{MaterialUvs, Mesh, MeshBuilder, Paint};
use crate::textures::Material;

const PLASTER: Paint = Paint::plain(Material::Plaster);
const THATCH: Paint = Paint::plain(Material::Thatch);
const TILE: Paint = Paint::plain(Material::RoofTile);
const WOOD: Paint = Paint::plain(Material::Wood);
const STONE: Paint = Paint::plain(Material::StoneBlock);

/// Деревянная балка фахверка на стене (тонкая коробка чуть перед стеной).
fn beam(b: &mut MeshBuilder, c: [f32; 3], s: [f32; 3]) {
    b.block(c, s, WOOD);
}

/// Крестьянский дом 6×5 м: оштукатуренные стены, соломенная крыша.
pub fn house(uvs: &MaterialUvs, detail: u8) -> Mesh {
    let mut b = MeshBuilder::new(uvs);
    let (w, d, h) = (6.0, 5.0, 3.0);
    b.block([0.0, h * 0.5, 0.0], [w, h, d], PLASTER);
    b.gable_roof([0.0, h, 0.0], w + 0.6, d + 0.6, 2.4, THATCH);
    if detail == 0 {
        b.block([0.0, 1.0, d * 0.5 + 0.03], [1.0, 2.0, 0.06], WOOD);
        for x in [-w * 0.5 + 0.1, w * 0.5 - 0.1] {
            beam(&mut b, [x, h * 0.5, d * 0.5 + 0.03], [0.2, h, 0.06]);
        }
        beam(&mut b, [0.0, h - 0.1, d * 0.5 + 0.03], [w, 0.2, 0.06]);
        b.block([1.8, 1.7, d * 0.5 + 0.03], [0.7, 0.6, 0.06], WOOD);
        b.block([-1.8, 1.7, d * 0.5 + 0.03], [0.7, 0.6, 0.06], WOOD);
        // Печная труба.
        b.block([1.5, h + 1.8, -1.0], [0.5, 1.6, 0.5], STONE);
    } else if detail == 1 {
        b.block([0.0, 1.0, d * 0.5 + 0.03], [1.0, 2.0, 0.06], WOOD);
    }
    b.finish()
}

/// Каменная церковь: неф с черепичной крышей и колокольня у входа.
pub fn church(uvs: &MaterialUvs, detail: u8) -> Mesh {
    let mut b = MeshBuilder::new(uvs);
    let (w, d, h) = (8.0, 16.0, 7.0);
    b.block([0.0, h * 0.5, -2.0], [w, h, d], STONE);
    b.gable_roof([0.0, h, -2.0], w + 0.6, d + 0.4, 3.5, TILE);
    // Колокольня.
    let tw = 4.0;
    b.block([0.0, 7.5, 7.0], [tw, 15.0, tw], STONE);
    b.pyramid([0.0, 15.0, 7.0], tw + 0.4, tw + 0.4, 5.0, TILE);
    if detail == 0 {
        // Портал, окна-бойницы колокольни, апсида за алтарём.
        b.block([0.0, 1.6, 9.03], [1.6, 3.2, 0.08], WOOD);
        for (x, z) in [(0.0, 9.03), (0.0, 4.97)] {
            b.block([x, 12.5, z], [0.8, 1.6, 0.08], WOOD);
        }
        b.cylinder([0.0, 0.0, -10.0], 3.0, 5.5, 8, STONE);
        b.frustum([0.0, 5.5, -10.0], 3.3, 0.0, 2.5, 8, TILE);
        // Контрфорсы вдоль нефа.
        for z in [-8.0, -4.0, 0.0, 4.0] {
            for x in [-w * 0.5 - 0.3, w * 0.5 + 0.3] {
                b.block([x, 2.5, z - 2.0], [0.6, 5.0, 0.8], STONE);
            }
        }
    } else if detail == 1 {
        b.block([0.0, 1.6, 9.03], [1.6, 3.2, 0.08], WOOD);
        b.block([0.0, 2.75, -10.5], [5.0, 5.5, 3.0], STONE);
    }
    b.finish()
}

/// Деревянная сторожевая башня на четырёх столбах с площадкой и крышей.
pub fn watchtower(uvs: &MaterialUvs, detail: u8) -> Mesh {
    let mut b = MeshBuilder::new(uvs);
    let (s, h) = (3.0, 8.0);
    if detail < 2 {
        let seg = if detail == 0 { 6 } else { 4 };
        for (x, z) in [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
            b.cylinder([x * s * 0.4, 0.0, z * s * 0.4], 0.15, h, seg, WOOD);
        }
        b.block([0.0, h, 0.0], [s + 0.6, 0.3, s + 0.6], WOOD);
        b.block([0.0, h + 0.65, s * 0.5 + 0.25], [s + 0.6, 1.0, 0.1], WOOD);
        b.block([0.0, h + 0.65, -s * 0.5 - 0.25], [s + 0.6, 1.0, 0.1], WOOD);
        if detail == 0 {
            b.block([s * 0.5 + 0.25, h + 0.65, 0.0], [0.1, 1.0, s + 0.4], WOOD);
            b.block([-s * 0.5 - 0.25, h + 0.65, 0.0], [0.1, 1.0, s + 0.4], WOOD);
            // Лестница.
            b.block([0.0, h * 0.5, s * 0.5 + 0.6], [0.6, h, 0.1], WOOD);
        }
    } else {
        b.block([0.0, h * 0.5, 0.0], [s * 0.8, h, s * 0.8], WOOD);
    }
    b.pyramid([0.0, h + 2.2, 0.0], s + 1.0, s + 1.0, 1.8, THATCH);
    if detail < 2 {
        for (x, z) in [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
            b.block(
                [x * s * 0.45, h + 1.2, z * s * 0.45],
                [0.15, 2.0, 0.15],
                WOOD,
            );
        }
    }
    b.finish()
}

/// Амбар 10×7 м: дощатые стены и высокая соломенная крыша.
pub fn barn(uvs: &MaterialUvs, detail: u8) -> Mesh {
    let mut b = MeshBuilder::new(uvs);
    let (w, d, h) = (7.0, 10.0, 3.5);
    b.block([0.0, h * 0.5, 0.0], [w, h, d], WOOD);
    b.gable_roof([0.0, h, 0.0], w + 0.8, d + 0.6, 3.5, THATCH);
    if detail < 2 {
        b.block([0.0, 1.5, d * 0.5 + 0.04], [3.0, 3.0, 0.08], WOOD);
    }
    if detail == 0 {
        b.block([0.0, 1.5, d * 0.5 + 0.09], [3.2, 0.2, 0.06], WOOD);
        b.block([0.0, 3.0, d * 0.5 + 0.09], [3.2, 0.2, 0.06], WOOD);
    }
    b.finish()
}
