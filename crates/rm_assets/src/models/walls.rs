//! Городские укрепления: прясло каменной стены, круглая башня,
//! надвратная башня и деревянный частокол.
//!
//! Сегменты стыкуются по оси x: длина прясла [`WALL_SEGMENT_LENGTH`],
//! центр в начале координат, «наружная» сторона — +z. Город собирает
//! из них кольцо стен, ставя экземпляры с поворотом (`rm_city`).

use crate::mesh::{MaterialUvs, Mesh, MeshBuilder, Paint};
use crate::textures::Material;

const STONE: Paint = Paint::plain(Material::StoneBlock);
const WOOD: Paint = Paint::plain(Material::Wood);
const TILE: Paint = Paint::plain(Material::RoofTile);

/// Длина одного прясла стены, м.
pub const WALL_SEGMENT_LENGTH: f32 = 12.0;
/// Высота стены до парапета, м.
pub const WALL_HEIGHT: f32 = 8.0;
/// Толщина стены, м.
pub const WALL_THICKNESS: f32 = 2.5;

/// Зубцы парапета вдоль x на высоте `y` поверх стены длиной `len`.
fn merlons(b: &mut MeshBuilder, len: f32, y: f32, z: f32, count: u32) {
    let step = len / count as f32;
    for i in 0..count {
        let x = -len * 0.5 + step * (i as f32 + 0.5);
        b.block([x, y + 0.6, z], [step * 0.55, 1.2, 0.5], STONE);
    }
}

/// Прясло стены. Стена собрана из блоков по 3 м, чтобы текстура кладки
/// не растягивалась (каждая грань — целый кусок атласа).
pub fn wall_segment(uvs: &MaterialUvs, detail: u8) -> Mesh {
    let mut b = MeshBuilder::new(uvs);
    let len = WALL_SEGMENT_LENGTH;
    let blocks = if detail == 0 { 4 } else { 1 };
    let bl = len / blocks as f32;
    for i in 0..blocks {
        let x = -len * 0.5 + bl * (i as f32 + 0.5);
        b.block(
            [x, WALL_HEIGHT * 0.5, 0.0],
            [bl, WALL_HEIGHT, WALL_THICKNESS],
            STONE,
        );
    }
    match detail {
        0 => {
            merlons(&mut b, len, WALL_HEIGHT, WALL_THICKNESS * 0.5 - 0.25, 8);
            // Низкий бортик со стороны города.
            b.block(
                [0.0, WALL_HEIGHT + 0.4, -WALL_THICKNESS * 0.5 + 0.15],
                [len, 0.8, 0.3],
                STONE,
            );
        }
        1 => merlons(&mut b, len, WALL_HEIGHT, WALL_THICKNESS * 0.5 - 0.25, 4),
        _ => {}
    }
    b.finish()
}

/// Круглая башня стены с зубцами и конической крышей.
pub fn wall_tower(uvs: &MaterialUvs, detail: u8) -> Mesh {
    let mut b = MeshBuilder::new(uvs);
    let (r, h) = (4.0, WALL_HEIGHT + 4.0);
    let seg = [12, 8, 6][detail.min(2) as usize];
    // Ствол башни из двух ярусов, чтобы кладка не тянулась на 12 м.
    if detail == 0 {
        b.cylinder([0.0, 0.0, 0.0], r, h * 0.5, seg, STONE);
        b.cylinder([0.0, h * 0.5, 0.0], r, h * 0.5, seg, STONE);
        for i in 0..seg {
            let a = (i as f32 + 0.5) / seg as f32 * std::f32::consts::TAU;
            let (s, c) = rm_core::dmath::sin_cos(a);
            b.block(
                [c * (r - 0.3), h + 0.6, s * (r - 0.3)],
                [0.9, 1.2, 0.9],
                STONE,
            );
        }
    } else {
        b.cylinder([0.0, 0.0, 0.0], r, h, seg, STONE);
    }
    b.frustum([0.0, h + 1.2, 0.0], r + 0.5, 0.0, 5.0, seg, TILE);
    b.finish()
}

/// Надвратная башня: две квадратные башни и арка над проездом с воротами.
pub fn gatehouse(uvs: &MaterialUvs, detail: u8) -> Mesh {
    let mut b = MeshBuilder::new(uvs);
    let (tw, h) = (5.0, WALL_HEIGHT + 5.0);
    let gap = 4.0;
    for side in [-1.0, 1.0] {
        let x = side * (gap * 0.5 + tw * 0.5);
        b.block([x, h * 0.5, 0.0], [tw, h, tw], STONE);
        if detail == 0 {
            merlons(&mut b, tw, h, tw * 0.5 - 0.25, 3);
        }
        if detail < 2 {
            b.pyramid([x, h + 1.2, 0.0], tw + 0.4, tw + 0.4, 3.0, TILE);
        }
    }
    // Перемычка над проездом.
    b.block([0.0, h - 2.5, 0.0], [gap, 5.0, tw], STONE);
    // Створки ворот.
    b.block(
        [0.0, (h - 5.0) * 0.5, tw * 0.5 - 0.3],
        [gap, h - 5.0, 0.3],
        WOOD,
    );
    if detail == 0 {
        // Решётка-герса перед воротами.
        for i in 0..5 {
            let x = -gap * 0.5 + 0.4 + i as f32 * (gap - 0.8) / 4.0;
            b.block(
                [x, (h - 5.0) * 0.5, tw * 0.5 + 0.1],
                [0.12, h - 5.0, 0.12],
                WOOD,
            );
        }
    }
    b.finish()
}

/// Деревянный частокол длиной [`WALL_SEGMENT_LENGTH`]: заострённые брёвна.
pub fn palisade(uvs: &MaterialUvs, detail: u8) -> Mesh {
    let mut b = MeshBuilder::new(uvs);
    let len = WALL_SEGMENT_LENGTH;
    let h = 4.0;
    if detail == 2 {
        b.block([0.0, h * 0.5, 0.0], [len, h, 0.5], WOOD);
        return b.finish();
    }
    let (logs, seg) = if detail == 0 { (24, 6) } else { (12, 4) };
    let step = len / logs as f32;
    for i in 0..logs {
        let x = -len * 0.5 + step * (i as f32 + 0.5);
        let r = step * 0.5;
        b.cylinder([x, 0.0, 0.0], r, h, seg, WOOD);
        b.frustum([x, h, 0.0], r, 0.0, 0.6, seg, WOOD);
    }
    b.finish()
}
