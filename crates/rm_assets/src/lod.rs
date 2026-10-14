//! LOD-цепочки с жёсткими порогами и импосторы (хак H-15).
//!
//! У модели несколько версий: чем дальше она от камеры, тем проще версия,
//! а совсем далеко — плоская картинка (импостор). Переход резкий, по
//! одному сравнению расстояния с порогом, без плавного смешивания:
//! так дёшево, и на расстоянии, где происходит смена, разница почти не видна.
//! Дальше последнего порога модель не рисуется вовсе.
//!
//! Рендер (`rm_crowd`, `rm_city_render`) раскладывает экземпляры по
//! уровням и рисует каждый уровень одним instanced-вызовом: один вызов
//! на (тип модели × уровень LOD), как требует `docs/adr/budgets.md`.

use crate::atlas::UvRect;
use crate::error::{AssetError, Result};
use crate::mesh::{Mesh, Vertex};
use crate::models::Category;

/// Что рисуется на уровне LOD.
#[derive(Debug, Clone, PartialEq)]
pub enum LodGeometry {
    /// Обычная модель.
    Mesh(Mesh),
    /// Картинка размером `width × height` метров, стоящая на земле.
    /// `uv` — где она лежит в атласе импосторов.
    Impostor { width: f32, height: f32, uv: UvRect },
}

impl LodGeometry {
    pub fn triangle_count(&self) -> usize {
        match self {
            LodGeometry::Mesh(m) => m.triangle_count(),
            LodGeometry::Impostor { .. } => 2,
        }
    }
}

/// Один уровень: рисуется, пока расстояние меньше `max_distance`.
#[derive(Debug, Clone, PartialEq)]
pub struct LodLevel {
    pub max_distance: f32,
    pub geometry: LodGeometry,
}

/// Цепочка уровней одной модели, от подробного к простому.
#[derive(Debug, Clone, PartialEq)]
pub struct LodChain {
    pub levels: Vec<LodLevel>,
}

impl LodChain {
    /// Какой уровень рисовать на расстоянии `distance` метров.
    /// `bias` — множитель порогов из настроек качества (1.0 — как есть,
    /// 0.5 — переключаться вдвое раньше на слабом железе).
    /// `None` — модель дальше последнего порога и не рисуется.
    pub fn select(&self, distance: f32, bias: f32) -> Option<usize> {
        self.levels
            .iter()
            .position(|l| distance < l.max_distance * bias)
    }

    /// Расстояние, дальше которого модель не рисуется.
    pub fn cull_distance(&self) -> f32 {
        self.levels.last().map_or(0.0, |l| l.max_distance)
    }

    /// Правила цепочки: пороги строго растут, треугольников не больше,
    /// чем на предыдущем уровне, импостор может быть только последним.
    pub fn validate(&self) -> Result<()> {
        if self.levels.is_empty() {
            return Err(AssetError::Invalid("пустая LOD-цепочка".into()));
        }
        let mut prev_dist = 0.0;
        let mut prev_tris = usize::MAX;
        for (i, l) in self.levels.iter().enumerate() {
            if l.max_distance.partial_cmp(&prev_dist) != Some(std::cmp::Ordering::Greater) {
                return Err(AssetError::Invalid(format!(
                    "LOD {i}: порог {} не больше предыдущего {prev_dist}",
                    l.max_distance
                )));
            }
            let tris = l.geometry.triangle_count();
            if tris > prev_tris {
                return Err(AssetError::Invalid(format!(
                    "LOD {i}: {tris} треугольников больше, чем на предыдущем уровне ({prev_tris})"
                )));
            }
            if let LodGeometry::Mesh(m) = &l.geometry {
                m.validate()?;
            } else if i + 1 != self.levels.len() {
                return Err(AssetError::Invalid(format!(
                    "LOD {i}: импостор не последний в цепочке"
                )));
            }
            prev_dist = l.max_distance;
            prev_tris = tris;
        }
        Ok(())
    }
}

/// Пороги переключения для каждой группы моделей, метры:
/// `[конец LOD0, конец LOD1, конец LOD2, конец импостора]`.
///
/// Это технические значения по умолчанию. По `docs/adr/modding-contract.md`
/// их переопределяет файл `descr_rm_lod.txt` (его разбор — задача `rm_data`),
/// чтобы игрок мог подогнать дальность под своё железо.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LodThresholds {
    pub soldier: [f32; 4],
    pub building: [f32; 4],
    pub wall: [f32; 4],
    pub tree: [f32; 4],
}

impl Default for LodThresholds {
    fn default() -> Self {
        Self {
            soldier: [30.0, 80.0, 200.0, 1200.0],
            building: [80.0, 250.0, 600.0, 3000.0],
            wall: [100.0, 300.0, 800.0, 3000.0],
            tree: [40.0, 120.0, 300.0, 2000.0],
        }
    }
}

impl LodThresholds {
    pub fn for_category(&self, c: Category) -> [f32; 4] {
        match c {
            Category::Soldier => self.soldier,
            Category::Building => self.building,
            Category::Wall => self.wall,
            Category::Tree => self.tree,
        }
    }
}

/// Сетка импостора: прямоугольник в плоскости x–y (смотрит на +z), низом на земле.
/// `crossed` — добавить второй прямоугольник под 90° (так делают дальние деревья:
/// крест из двух картинок выглядит объёмно с любой стороны без поворота к камере).
/// Солдатам крест не нужен: рендер поворачивает их картинку к камере сам.
pub fn impostor_mesh(width: f32, height: f32, uv: UvRect, crossed: bool) -> Mesh {
    let hw = width * 0.5;
    let mut mesh = Mesh::default();
    let mut push_quad = |corners: [[f32; 3]; 4], normal: [f32; 3]| {
        let base = mesh.vertices.len() as u32;
        let tex = [[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]];
        for (p, t) in corners.iter().zip(tex) {
            mesh.vertices.push(Vertex {
                pos: *p,
                normal,
                uv: uv.map(t),
                tint: 0.0,
            });
        }
        mesh.indices
            .extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    };
    push_quad(
        [
            [-hw, 0.0, 0.0],
            [hw, 0.0, 0.0],
            [hw, height, 0.0],
            [-hw, height, 0.0],
        ],
        [0.0, 0.0, 1.0],
    );
    if crossed {
        push_quad(
            [
                [0.0, 0.0, hw],
                [0.0, 0.0, -hw],
                [0.0, height, -hw],
                [0.0, height, hw],
            ],
            [1.0, 0.0, 0.0],
        );
    }
    mesh
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mesh::MaterialUvs;
    use crate::models::ModelId;

    fn chain() -> LodChain {
        let uvs = MaterialUvs::identity();
        let mut levels: Vec<LodLevel> = (0..3)
            .map(|d| LodLevel {
                max_distance: [30.0, 80.0, 200.0][d as usize],
                geometry: LodGeometry::Mesh(ModelId::Spearman.build(&uvs, d)),
            })
            .collect();
        levels.push(LodLevel {
            max_distance: 1200.0,
            geometry: LodGeometry::Impostor {
                width: 1.0,
                height: 2.0,
                uv: UvRect::FULL,
            },
        });
        LodChain { levels }
    }

    #[test]
    fn hard_thresholds() {
        let c = chain();
        c.validate().unwrap();
        assert_eq!(c.select(0.0, 1.0), Some(0));
        assert_eq!(c.select(29.99, 1.0), Some(0));
        assert_eq!(c.select(30.0, 1.0), Some(1));
        assert_eq!(c.select(199.0, 1.0), Some(2));
        assert_eq!(c.select(500.0, 1.0), Some(3));
        assert_eq!(c.select(1200.0, 1.0), None);
        // Слабое железо: всё переключается вдвое раньше.
        assert_eq!(c.select(20.0, 0.5), Some(1));
        assert_eq!(c.cull_distance(), 1200.0);
    }

    #[test]
    fn broken_chains_are_rejected() {
        let mut c = chain();
        c.levels.swap(0, 1);
        assert!(c.validate().is_err());
        let mut c = chain();
        c.levels[1].max_distance = 30.0;
        assert!(c.validate().is_err());
        let mut c = chain();
        let imp = c.levels.pop().unwrap();
        c.levels.insert(1, imp);
        assert!(c.validate().is_err());
    }

    #[test]
    fn impostor_mesh_shapes() {
        let flat = impostor_mesh(2.0, 3.0, UvRect::FULL, false);
        assert_eq!(flat.triangle_count(), 2);
        flat.validate().unwrap();
        let cross = impostor_mesh(2.0, 3.0, UvRect::FULL, true);
        assert_eq!(cross.triangle_count(), 4);
        assert_eq!(cross.bounds(), ([-1.0, 0.0, -1.0], [1.0, 3.0, 1.0]));
    }
}
