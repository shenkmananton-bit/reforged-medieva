//! Солдаты и всадники. Рост пехотинца около 1,8 м, как в `rm_app`.
//!
//! Каждый уровень детализации собирается отдельно, а не упрощается
//! автоматически: так проще держать жёсткие бюджеты треугольников и
//! узнаваемый силуэт на любом расстоянии.
//!
//! * `detail 0` — всё: руки, ноги, щит, шлем, оружие, восьмигранные цилиндры;
//! * `detail 1` — руки слиты с туловищем, цилиндры заменены коробками;
//! * `detail 2` — три-четыре коробки: силуэт и цвет фракции.

use crate::mesh::{MaterialUvs, Mesh, MeshBuilder, Paint, Tint};
use crate::textures::Material;

const SKIN: Paint = Paint::plain(Material::Skin);
const LEATHER: Paint = Paint::plain(Material::Leather);
const STEEL: Paint = Paint::plain(Material::Steel);
const WOOD: Paint = Paint::plain(Material::Wood);
const HIDE: Paint = Paint::plain(Material::HorseHide);
const TUNIC: Paint = Paint::team(Material::Cloth, Tint::Primary);
const TRIM: Paint = Paint::team(Material::Cloth, Tint::Secondary);

/// Какое у пехотинца оружие.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kit {
    Club,
    Spear,
    Sword,
    Crossbow,
}

/// Голова и шлем: у пехоты на высоте ~1,6 м, у всадника выше.
fn head(b: &mut MeshBuilder, y: f32, z: f32, detail: u8, helmet: Paint) {
    if detail == 0 {
        b.cylinder([0.0, y - 0.12, z], 0.12, 0.24, 8, SKIN);
        b.frustum([0.0, y + 0.08, z], 0.15, 0.04, 0.16, 8, helmet);
    } else {
        b.cuboid([0.0, y, z], [0.22, 0.24, 0.24], SKIN);
        b.cuboid([0.0, y + 0.14, z], [0.26, 0.08, 0.28], helmet);
    }
}

/// Тело пехотинца без оружия.
fn body(b: &mut MeshBuilder, detail: u8, helmet: Paint) {
    match detail {
        0 => {
            b.cuboid([-0.11, 0.4, 0.0], [0.15, 0.8, 0.18], LEATHER);
            b.cuboid([0.11, 0.4, 0.0], [0.15, 0.8, 0.18], LEATHER);
            b.cuboid([0.0, 1.12, 0.0], [0.46, 0.66, 0.28], TUNIC);
            b.cuboid([0.0, 0.86, 0.0], [0.48, 0.1, 0.3], TRIM);
            b.cuboid([-0.3, 1.12, 0.06], [0.12, 0.56, 0.14], TUNIC);
            b.cuboid([0.3, 1.12, 0.06], [0.12, 0.56, 0.14], TUNIC);
            head(b, 1.58, 0.0, 0, helmet);
        }
        1 => {
            b.cuboid([0.0, 0.4, 0.0], [0.38, 0.8, 0.18], LEATHER);
            b.cuboid([0.0, 1.12, 0.0], [0.7, 0.66, 0.28], TUNIC);
            head(b, 1.58, 0.0, 1, helmet);
        }
        _ => {
            b.cuboid([0.0, 0.4, 0.0], [0.38, 0.8, 0.2], LEATHER);
            b.cuboid([0.0, 1.25, 0.0], [0.6, 0.9, 0.3], TUNIC);
        }
    }
}

/// Пехотинец с оружием `kit`.
pub fn infantry(uvs: &MaterialUvs, kit: Kit, detail: u8) -> Mesh {
    let mut b = MeshBuilder::new(uvs);
    let helmet = if kit == Kit::Club { LEATHER } else { STEEL };
    body(&mut b, detail, helmet);
    let full = detail == 0;
    match kit {
        Kit::Club => {
            b.cuboid([0.32, 1.25, 0.35], [0.08, 0.08, 0.7], WOOD);
        }
        Kit::Spear => {
            b.cuboid([0.32, 1.25, 0.6], [0.06, 0.06, 2.6], WOOD);
            if full {
                b.cuboid([0.32, 1.25, 1.95], [0.1, 0.1, 0.2], STEEL);
            }
            if detail < 2 {
                b.cuboid([-0.38, 1.1, 0.2], [0.06, 0.6, 0.55], TRIM);
            }
        }
        Kit::Sword => {
            b.cuboid([0.34, 1.2, 0.45], [0.05, 0.06, 0.8], STEEL);
            if full {
                b.cuboid([0.34, 1.2, 0.08], [0.2, 0.05, 0.05], STEEL);
                // Каплевидный щит: широкая коробка и узкий низ.
                b.cuboid([-0.38, 1.18, 0.2], [0.06, 0.5, 0.5], TRIM);
                b.cuboid([-0.38, 0.87, 0.2], [0.06, 0.12, 0.28], TRIM);
            } else if detail == 1 {
                b.cuboid([-0.38, 1.1, 0.2], [0.06, 0.65, 0.5], TRIM);
            }
        }
        Kit::Crossbow => {
            b.cuboid([0.0, 1.25, 0.4], [0.1, 0.1, 0.6], WOOD);
            if detail < 2 {
                b.cuboid([0.0, 1.25, 0.62], [0.6, 0.06, 0.06], STEEL);
            }
            if full {
                b.cylinder([0.18, 1.0, -0.2], 0.06, 0.45, 6, LEATHER);
            }
        }
    }
    b.finish()
}

/// Всадник на коне с луком.
pub fn horseman(uvs: &MaterialUvs, detail: u8) -> Mesh {
    let mut b = MeshBuilder::new(uvs);
    match detail {
        0 => {
            for (x, z) in [(-0.2, 0.7), (0.2, 0.7), (-0.2, -0.7), (0.2, -0.7)] {
                b.cuboid([x, 0.45, z], [0.14, 0.9, 0.14], HIDE);
            }
            b.cuboid([0.0, 1.1, 0.0], [0.5, 0.5, 1.7], HIDE);
            b.cuboid([0.0, 1.0, 0.0], [0.56, 0.3, 1.2], TRIM);
            b.cuboid([0.0, 1.5, 0.95], [0.3, 0.6, 0.3], HIDE);
            b.cuboid([0.0, 1.75, 1.2], [0.24, 0.24, 0.5], HIDE);
            b.cuboid([0.0, 1.2, -0.95], [0.08, 0.5, 0.08], HIDE);
            b.cuboid([0.0, 1.75, -0.05], [0.42, 0.6, 0.28], TUNIC);
            b.cuboid([-0.28, 1.75, 0.05], [0.12, 0.5, 0.14], TUNIC);
            b.cuboid([0.28, 1.75, 0.05], [0.12, 0.5, 0.14], TUNIC);
            b.cuboid([-0.28, 1.3, 0.0], [0.12, 0.5, 0.16], LEATHER);
            b.cuboid([0.28, 1.3, 0.0], [0.12, 0.5, 0.16], LEATHER);
            head(&mut b, 2.17, -0.05, 0, STEEL);
            b.cuboid([-0.4, 1.8, 0.2], [0.05, 0.9, 0.06], WOOD);
        }
        1 => {
            b.cuboid([0.0, 0.45, 0.7], [0.5, 0.9, 0.14], HIDE);
            b.cuboid([0.0, 0.45, -0.7], [0.5, 0.9, 0.14], HIDE);
            b.cuboid([0.0, 1.1, 0.0], [0.56, 0.5, 1.7], TRIM);
            b.cuboid([0.0, 1.6, 1.05], [0.28, 0.5, 0.5], HIDE);
            b.cuboid([0.0, 1.75, -0.05], [0.7, 0.6, 0.28], TUNIC);
            head(&mut b, 2.17, -0.05, 1, STEEL);
            b.cuboid([-0.4, 1.8, 0.2], [0.05, 0.9, 0.06], WOOD);
        }
        _ => {
            b.cuboid([0.0, 0.45, 0.0], [0.5, 0.9, 1.6], HIDE);
            b.cuboid([0.0, 1.15, 0.1], [0.56, 0.5, 2.0], TRIM);
            b.cuboid([0.0, 1.9, -0.05], [0.5, 0.9, 0.3], TUNIC);
        }
    }
    b.finish()
}

/// Знамя отряда: древко и полотнище в цветах фракции.
pub fn banner(uvs: &MaterialUvs, detail: u8) -> Mesh {
    let mut b = MeshBuilder::new(uvs);
    if detail == 0 {
        b.cylinder([0.0, 0.0, 0.0], 0.04, 4.0, 6, WOOD);
        b.frustum([0.0, 4.0, 0.0], 0.06, 0.0, 0.2, 6, STEEL);
    } else {
        b.cuboid([0.0, 2.0, 0.0], [0.08, 4.0, 0.08], WOOD);
    }
    b.cuboid([0.0, 3.4, -0.75], [0.05, 1.1, 1.4], TUNIC);
    if detail < 2 {
        b.cuboid([0.0, 3.4, -0.75], [0.06, 0.3, 1.42], TRIM);
    }
    b.finish()
}
