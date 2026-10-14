//! Сетка треугольников рельефа.
//!
//! Хак H-01: карта — это не 3D-мир, а лист, приподнятый по сетке высот.
//! Треугольников немного (сетка в 4 раза реже точек высот): мелкий рельеф
//! рисуется не геометрией, а запечённой normal map (H-02). Вся сетка —
//! один буфер вершин и один вызов рисования. Вода — та же сетка,
//! прижатая к нулю: поверхность моря получается плоской сама собой.

use crate::map::CampaignMap;

/// Вершина: позиция в мире и координата в текстурах карты (0..1).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TerrainVertex {
    pub pos: [f32; 3],
    pub uv: [f32; 2],
}

#[derive(Debug, Clone)]
pub struct TerrainMesh {
    pub vertices: Vec<TerrainVertex>,
    pub indices: Vec<u32>,
}

/// Во сколько раз сетка треугольников реже сетки высот.
pub const MESH_STEP: u32 = 4;

impl TerrainMesh {
    pub fn build(map: &CampaignMap) -> Self {
        let cells = (map.config.size / MESH_STEP).max(2);
        let world = map.config.world_size();
        let side = cells + 1;
        let mut vertices = Vec::with_capacity((side * side) as usize);
        for j in 0..side {
            for i in 0..side {
                let (u, v) = (i as f32 / cells as f32, j as f32 / cells as f32);
                let (x, z) = (u * world, v * world);
                let y = map.display_height_at(x, z);
                vertices.push(TerrainVertex {
                    pos: [x, y, z],
                    uv: [u, v],
                });
            }
        }
        let mut indices = Vec::with_capacity((cells * cells * 6) as usize);
        for j in 0..cells {
            for i in 0..cells {
                let a = j * side + i;
                let (b, c, d) = (a + 1, a + side, a + side + 1);
                // Диагональ квадрата выбираем по рельефу: так гребни гор не «пилятся».
                let h = |k: u32| vertices[k as usize].pos[1];
                if (h(a) - h(d)).abs() < (h(b) - h(c)).abs() {
                    indices.extend_from_slice(&[a, c, d, a, d, b]);
                } else {
                    indices.extend_from_slice(&[a, c, b, b, c, d]);
                }
            }
        }
        Self { vertices, indices }
    }

    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::MapConfig;

    #[test]
    fn mesh_covers_the_map() {
        let m = CampaignMap::generate(&MapConfig {
            size: 128,
            regions: 8,
            ..MapConfig::default()
        });
        let mesh = TerrainMesh::build(&m);
        assert_eq!(mesh.vertices.len(), 33 * 33);
        assert_eq!(mesh.triangle_count(), 32 * 32 * 2);
        let last = mesh.vertices.last().unwrap();
        assert_eq!(last.uv, [1.0, 1.0]);
        assert_eq!(last.pos[0], 128.0);
        assert!(
            mesh.vertices.iter().all(|v| v.pos[1] >= 0.0),
            "вода не ниже нуля"
        );
        assert!(
            mesh.indices
                .iter()
                .all(|&i| (i as usize) < mesh.vertices.len())
        );
    }
}
