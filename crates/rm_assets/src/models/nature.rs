//! Деревья для поля битвы и окрестностей городов.
//!
//! Лес в битве — это тысячи одинаковых деревьев, поэтому дальний LOD
//! у них — импостор из двух скрещённых картинок (см. `lod.rs`).

use crate::mesh::{MaterialUvs, Mesh, MeshBuilder, Paint};
use crate::textures::Material;

const BARK: Paint = Paint::plain(Material::Bark);
const LEAVES: Paint = Paint::plain(Material::Foliage);

/// Ель: ствол и три яруса конусов хвои.
pub fn pine(uvs: &MaterialUvs, detail: u8) -> Mesh {
    let mut b = MeshBuilder::new(uvs);
    let seg = [8, 6, 4][detail.min(2) as usize];
    b.cylinder([0.0, 0.0, 0.0], 0.25, 2.0, seg.min(6), BARK);
    let tiers: &[(f32, f32, f32)] = match detail {
        0 => &[(1.5, 2.4, 3.0), (3.5, 1.9, 2.8), (5.3, 1.3, 2.6)],
        1 => &[(1.5, 2.3, 3.5), (4.2, 1.6, 3.4)],
        _ => &[(1.0, 2.2, 7.0)],
    };
    for &(y, r, h) in tiers {
        b.frustum([0.0, y, 0.0], r, 0.0, h, seg, LEAVES);
    }
    b.finish()
}

/// Дуб: толстый ствол и угловатая крона из коробок.
pub fn oak(uvs: &MaterialUvs, detail: u8) -> Mesh {
    let mut b = MeshBuilder::new(uvs);
    match detail {
        0 => {
            b.cylinder([0.0, 0.0, 0.0], 0.4, 3.5, 8, BARK);
            b.block([0.8, 3.2, 0.0], [1.6, 0.3, 0.3], BARK);
            b.cuboid([0.0, 5.0, 0.0], [4.5, 3.0, 4.5], LEAVES);
            b.cuboid([1.2, 6.3, 0.6], [2.6, 2.0, 2.6], LEAVES);
            b.cuboid([-1.0, 6.0, -0.9], [2.4, 2.2, 2.4], LEAVES);
            b.cuboid([0.0, 4.2, 1.6], [2.0, 1.4, 1.8], LEAVES);
        }
        1 => {
            b.cylinder([0.0, 0.0, 0.0], 0.4, 3.5, 5, BARK);
            b.cuboid([0.0, 5.2, 0.0], [4.6, 3.4, 4.6], LEAVES);
            b.cuboid([0.4, 6.8, 0.0], [2.8, 1.6, 2.8], LEAVES);
        }
        _ => {
            b.block([0.0, 1.75, 0.0], [0.7, 3.5, 0.7], BARK);
            b.cuboid([0.0, 5.4, 0.0], [4.8, 4.0, 4.8], LEAVES);
        }
    }
    b.finish()
}
