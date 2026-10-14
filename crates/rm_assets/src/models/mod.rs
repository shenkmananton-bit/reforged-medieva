//! Каталог процедурных моделей-заглушек.
//!
//! Каждая модель умеет собрать себя на трёх уровнях детализации
//! (`detail` 0, 1, 2). Четвёртый, самый дальний уровень — импостор:
//! картинка вместо геометрии, его делает [`crate::impostor`].

pub mod buildings;
pub mod nature;
pub mod soldiers;
pub mod walls;

use crate::mesh::{MaterialUvs, Mesh};
use soldiers::Kit;

/// Число уровней с геометрией (без импостора).
pub const MESH_LODS: u8 = 3;

/// К какой группе относится модель: от неё зависят пороги LOD.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Category {
    Soldier,
    Building,
    Wall,
    Tree,
}

/// Все модели, которые умеет делать конвейер.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ModelId {
    Levy,
    Spearman,
    Swordsman,
    Crossbowman,
    Horseman,
    Banner,
    House,
    Church,
    Watchtower,
    Barn,
    WallSegment,
    WallTower,
    Gatehouse,
    Palisade,
    Pine,
    Oak,
}

impl ModelId {
    pub const ALL: [ModelId; 16] = [
        ModelId::Levy,
        ModelId::Spearman,
        ModelId::Swordsman,
        ModelId::Crossbowman,
        ModelId::Horseman,
        ModelId::Banner,
        ModelId::House,
        ModelId::Church,
        ModelId::Watchtower,
        ModelId::Barn,
        ModelId::WallSegment,
        ModelId::WallTower,
        ModelId::Gatehouse,
        ModelId::Palisade,
        ModelId::Pine,
        ModelId::Oak,
    ];

    /// Имя модели в пакете и в файлах превью.
    pub fn name(self) -> &'static str {
        match self {
            ModelId::Levy => "levy",
            ModelId::Spearman => "spearman",
            ModelId::Swordsman => "swordsman",
            ModelId::Crossbowman => "crossbowman",
            ModelId::Horseman => "horseman",
            ModelId::Banner => "banner",
            ModelId::House => "house",
            ModelId::Church => "church",
            ModelId::Watchtower => "watchtower",
            ModelId::Barn => "barn",
            ModelId::WallSegment => "wall_segment",
            ModelId::WallTower => "wall_tower",
            ModelId::Gatehouse => "gatehouse",
            ModelId::Palisade => "palisade",
            ModelId::Pine => "pine",
            ModelId::Oak => "oak",
        }
    }

    pub fn from_name(name: &str) -> Option<ModelId> {
        Self::ALL.into_iter().find(|m| m.name() == name)
    }

    pub fn category(self) -> Category {
        match self {
            ModelId::Levy
            | ModelId::Spearman
            | ModelId::Swordsman
            | ModelId::Crossbowman
            | ModelId::Horseman
            | ModelId::Banner => Category::Soldier,
            ModelId::House | ModelId::Church | ModelId::Watchtower | ModelId::Barn => {
                Category::Building
            }
            ModelId::WallSegment | ModelId::WallTower | ModelId::Gatehouse | ModelId::Palisade => {
                Category::Wall
            }
            ModelId::Pine | ModelId::Oak => Category::Tree,
        }
    }

    /// Собирает модель на уровне детализации `detail` (0 — самый подробный).
    pub fn build(self, uvs: &MaterialUvs, detail: u8) -> Mesh {
        let d = detail.min(MESH_LODS - 1);
        match self {
            ModelId::Levy => soldiers::infantry(uvs, Kit::Club, d),
            ModelId::Spearman => soldiers::infantry(uvs, Kit::Spear, d),
            ModelId::Swordsman => soldiers::infantry(uvs, Kit::Sword, d),
            ModelId::Crossbowman => soldiers::infantry(uvs, Kit::Crossbow, d),
            ModelId::Horseman => soldiers::horseman(uvs, d),
            ModelId::Banner => soldiers::banner(uvs, d),
            ModelId::House => buildings::house(uvs, d),
            ModelId::Church => buildings::church(uvs, d),
            ModelId::Watchtower => buildings::watchtower(uvs, d),
            ModelId::Barn => buildings::barn(uvs, d),
            ModelId::WallSegment => walls::wall_segment(uvs, d),
            ModelId::WallTower => walls::wall_tower(uvs, d),
            ModelId::Gatehouse => walls::gatehouse(uvs, d),
            ModelId::Palisade => walls::palisade(uvs, d),
            ModelId::Pine => nature::pine(uvs, d),
            ModelId::Oak => nature::oak(uvs, d),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_model_builds_valid_meshes_with_falling_detail() {
        let uvs = MaterialUvs::identity();
        for m in ModelId::ALL {
            let mut prev = usize::MAX;
            for d in 0..MESH_LODS {
                let mesh = m.build(&uvs, d);
                mesh.validate()
                    .unwrap_or_else(|e| panic!("{} lod{d}: {e}", m.name()));
                let tris = mesh.triangle_count();
                assert!(
                    tris < prev,
                    "{} lod{d}: {tris} треугольников не меньше {prev}",
                    m.name()
                );
                prev = tris;
            }
            assert_eq!(ModelId::from_name(m.name()), Some(m));
        }
    }

    #[test]
    fn soldiers_fit_triangle_budget() {
        // Бюджет на одного солдата: 10 000 на экране, большинство на дальних LOD.
        let uvs = MaterialUvs::identity();
        let budget = [400, 120, 40];
        for m in ModelId::ALL
            .into_iter()
            .filter(|m| m.category() == Category::Soldier)
        {
            for d in 0..MESH_LODS {
                let tris = m.build(&uvs, d).triangle_count();
                assert!(tris <= budget[d as usize], "{} lod{d}: {tris}", m.name());
            }
        }
    }

    #[test]
    fn models_stand_on_the_ground() {
        let uvs = MaterialUvs::identity();
        for m in ModelId::ALL {
            let (lo, hi) = m.build(&uvs, 0).bounds();
            assert!(
                lo[1].abs() < 1e-4,
                "{} висит или проваливается: {}",
                m.name(),
                lo[1]
            );
            assert!(hi[1] > 1.0);
        }
    }
}
