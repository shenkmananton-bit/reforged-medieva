//! Индексированные low-poly меши и построитель простых тел.
//!
//! Система координат как в `rm_app`: модель стоит на земле (y = 0),
//! смотрит вдоль +z, x — вправо; единица — метр. Нормали плоские
//! (у каждой грани свои вершины) — это даёт «рубленый» low-poly вид
//! и правильный свет без сглаживания.
//!
//! Каждая грань натягивается на свой кусок атласа целиком (UV 0..1 внутри
//! куска). Поэтому длинные стены собираются из нескольких коротких блоков,
//! а не из одной растянутой коробки.

use rm_core::dmath;

use crate::atlas::{Atlas, UvRect};
use crate::error::{AssetError, Result};
use crate::textures::Material;

/// Чем красить часть модели поверх текстуры.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tint {
    /// Текстура как есть.
    #[default]
    None = 0,
    /// Умножить на основной цвет фракции.
    Primary = 1,
    /// Умножить на второй цвет фракции.
    Secondary = 2,
}

impl Tint {
    pub fn from_f32(v: f32) -> Tint {
        if v > 1.5 {
            Tint::Secondary
        } else if v > 0.5 {
            Tint::Primary
        } else {
            Tint::None
        }
    }
}

/// Вершина меша. 36 байт, `#[repr(C)]` — можно сразу копировать в буфер GPU.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vertex {
    pub pos: [f32; 3],
    pub normal: [f32; 3],
    /// Координаты в атласе материалов.
    pub uv: [f32; 2],
    /// [`Tint`] как число: 0, 1 или 2 (так же, как альфа цвета в `rm_app`).
    pub tint: f32,
}

/// Меш: вершины и тройки индексов (треугольники против часовой стрелки снаружи).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Mesh {
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
}

impl Mesh {
    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }

    /// Ограничивающая коробка: (минимум, максимум).
    pub fn bounds(&self) -> ([f32; 3], [f32; 3]) {
        let mut lo = [f32::MAX; 3];
        let mut hi = [f32::MIN; 3];
        for v in &self.vertices {
            for a in 0..3 {
                lo[a] = lo[a].min(v.pos[a]);
                hi[a] = hi[a].max(v.pos[a]);
            }
        }
        if self.vertices.is_empty() {
            return ([0.0; 3], [0.0; 3]);
        }
        (lo, hi)
    }

    /// Дописывает другой меш в конец этого.
    pub fn append(&mut self, other: &Mesh) {
        let base = self.vertices.len() as u32;
        self.vertices.extend_from_slice(&other.vertices);
        self.indices.extend(other.indices.iter().map(|i| i + base));
    }

    /// Проверяет, что меш можно отдавать рендеру.
    pub fn validate(&self) -> Result<()> {
        if self.indices.is_empty() || !self.indices.len().is_multiple_of(3) {
            return Err(AssetError::Invalid(format!(
                "число индексов {} не делится на 3 или равно нулю",
                self.indices.len()
            )));
        }
        let n = self.vertices.len() as u32;
        if let Some(i) = self.indices.iter().find(|&&i| i >= n) {
            return Err(AssetError::Invalid(format!(
                "индекс {i} за пределами {n} вершин"
            )));
        }
        for v in &self.vertices {
            let len2: f32 = v.normal.iter().map(|c| c * c).sum();
            if (len2 - 1.0).abs() > 1e-3 || v.pos.iter().any(|c| !c.is_finite()) {
                return Err(AssetError::Invalid(format!("плохая вершина {v:?}")));
            }
        }
        Ok(())
    }
}

/// Куда в атласе указывает каждый материал.
#[derive(Debug, Clone, PartialEq)]
pub struct MaterialUvs {
    rects: [UvRect; Material::COUNT],
}

impl MaterialUvs {
    /// Берёт куски `mat/<имя>` из атласа материалов.
    pub fn from_atlas(atlas: &Atlas) -> Result<Self> {
        let mut rects = [UvRect::default(); Material::COUNT];
        for m in Material::ALL {
            let name = format!("mat/{}", m.name());
            rects[m.index()] = atlas
                .uv(&name)
                .ok_or_else(|| AssetError::Atlas(format!("в атласе нет «{name}»")))?;
        }
        Ok(Self { rects })
    }

    /// Все материалы смотрят на весь атлас (для тестов без настоящего атласа).
    pub fn identity() -> Self {
        Self {
            rects: [UvRect::FULL; Material::COUNT],
        }
    }

    pub fn get(&self, m: Material) -> UvRect {
        self.rects[m.index()]
    }
}

/// Краска для части модели: материал атласа и окраска фракции.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Paint {
    pub material: Material,
    pub tint: Tint,
}

impl Paint {
    pub const fn plain(material: Material) -> Self {
        Self {
            material,
            tint: Tint::None,
        }
    }

    pub const fn team(material: Material, tint: Tint) -> Self {
        Self { material, tint }
    }
}

/// Собирает меш из коробок, цилиндров, конусов и двускатных крыш.
pub struct MeshBuilder<'a> {
    mesh: Mesh,
    uvs: &'a MaterialUvs,
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn normalize(a: [f32; 3]) -> [f32; 3] {
    let len = dmath::sqrt(a[0] * a[0] + a[1] * a[1] + a[2] * a[2]);
    if len == 0.0 {
        return [0.0, 1.0, 0.0];
    }
    [a[0] / len, a[1] / len, a[2] / len]
}

impl<'a> MeshBuilder<'a> {
    pub fn new(uvs: &'a MaterialUvs) -> Self {
        Self {
            mesh: Mesh::default(),
            uvs,
        }
    }

    pub fn finish(self) -> Mesh {
        self.mesh
    }

    /// Многоугольник (выпуклый, вершины против часовой стрелки, если смотреть снаружи).
    /// `uv` — локальные координаты каждой вершины внутри куска атласа.
    pub fn polygon(&mut self, pts: &[[f32; 3]], uv: &[[f32; 2]], paint: Paint) {
        debug_assert!(pts.len() >= 3 && pts.len() == uv.len());
        let n = normalize(cross(sub(pts[1], pts[0]), sub(pts[2], pts[0])));
        let rect = self.uvs.get(paint.material);
        let base = self.mesh.vertices.len() as u32;
        for (p, t) in pts.iter().zip(uv) {
            self.mesh.vertices.push(Vertex {
                pos: *p,
                normal: n,
                uv: rect.map(*t),
                tint: paint.tint as u8 as f32,
            });
        }
        for i in 1..pts.len() as u32 - 1 {
            self.mesh
                .indices
                .extend_from_slice(&[base, base + i, base + i + 1]);
        }
    }

    /// Четырёхугольник `a b c d` против часовой стрелки; `a` — левый нижний угол текстуры.
    pub fn quad(&mut self, a: [f32; 3], b: [f32; 3], c: [f32; 3], d: [f32; 3], paint: Paint) {
        self.polygon(
            &[a, b, c, d],
            &[[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]],
            paint,
        );
    }

    /// Коробка с центром `c` и размерами `s` по осям.
    pub fn cuboid(&mut self, c: [f32; 3], s: [f32; 3], paint: Paint) {
        self.cuboid_faces(c, s, paint, true);
    }

    /// Коробка без нижней грани (стоит на земле, низ никто не видит).
    pub fn block(&mut self, c: [f32; 3], s: [f32; 3], paint: Paint) {
        self.cuboid_faces(c, s, paint, false);
    }

    fn cuboid_faces(&mut self, c: [f32; 3], s: [f32; 3], paint: Paint, bottom: bool) {
        let (x0, x1) = (c[0] - s[0] * 0.5, c[0] + s[0] * 0.5);
        let (y0, y1) = (c[1] - s[1] * 0.5, c[1] + s[1] * 0.5);
        let (z0, z1) = (c[2] - s[2] * 0.5, c[2] + s[2] * 0.5);
        // Перед (+z), зад (-z), право (+x), лево (-x), верх, низ.
        self.quad(
            [x0, y0, z1],
            [x1, y0, z1],
            [x1, y1, z1],
            [x0, y1, z1],
            paint,
        );
        self.quad(
            [x1, y0, z0],
            [x0, y0, z0],
            [x0, y1, z0],
            [x1, y1, z0],
            paint,
        );
        self.quad(
            [x1, y0, z1],
            [x1, y0, z0],
            [x1, y1, z0],
            [x1, y1, z1],
            paint,
        );
        self.quad(
            [x0, y0, z0],
            [x0, y0, z1],
            [x0, y1, z1],
            [x0, y1, z0],
            paint,
        );
        self.quad(
            [x0, y1, z1],
            [x1, y1, z1],
            [x1, y1, z0],
            [x0, y1, z0],
            paint,
        );
        if bottom {
            self.quad(
                [x0, y0, z0],
                [x1, y0, z0],
                [x1, y0, z1],
                [x0, y0, z1],
                paint,
            );
        }
    }

    /// Вертикальный цилиндр (призма из `segments` граней) с основанием в `base`.
    pub fn cylinder(
        &mut self,
        base: [f32; 3],
        radius: f32,
        height: f32,
        segments: u32,
        paint: Paint,
    ) {
        self.frustum(base, radius, radius, height, segments, paint);
    }

    /// Усечённый конус: радиус внизу `r0`, вверху `r1`. При `r1 = 0` — конус.
    pub fn frustum(
        &mut self,
        base: [f32; 3],
        r0: f32,
        r1: f32,
        height: f32,
        segments: u32,
        paint: Paint,
    ) {
        let segments = segments.max(3);
        let ring = |r: f32, y: f32| -> Vec<[f32; 3]> {
            (0..segments)
                .map(|i| {
                    let a = i as f32 / segments as f32 * std::f32::consts::TAU;
                    let (s, c) = dmath::sin_cos(a);
                    [base[0] + c * r, base[1] + y, base[2] + s * r]
                })
                .collect()
        };
        let lo = ring(r0, 0.0);
        let hi = ring(r1, height);
        for i in 0..segments as usize {
            let j = (i + 1) % segments as usize;
            let (u0, u1) = (i as f32 / segments as f32, (i + 1) as f32 / segments as f32);
            if r1 > 0.0 {
                // Обход j → i снаружи даёт нормаль наружу (ось вверх, угол растёт к +z).
                self.polygon(
                    &[lo[j], lo[i], hi[i], hi[j]],
                    &[[u1, 1.0], [u0, 1.0], [u0, 0.0], [u1, 0.0]],
                    paint,
                );
            } else {
                let tip = [base[0], base[1] + height, base[2]];
                self.polygon(
                    &[lo[j], lo[i], tip],
                    &[[u1, 1.0], [u0, 1.0], [(u0 + u1) * 0.5, 0.0]],
                    paint,
                );
            }
        }
        if r1 > 0.0 {
            let mut top = hi.clone();
            top.reverse();
            let uv: Vec<[f32; 2]> = top
                .iter()
                .map(|p| {
                    [
                        0.5 + (p[0] - base[0]) / (2.0 * r1),
                        0.5 + (p[2] - base[2]) / (2.0 * r1),
                    ]
                })
                .collect();
            self.polygon(&top, &uv, paint);
        }
    }

    /// Двускатная крыша: основание `w × d` на высоте `base[1]`, конёк вдоль z высотой `h`.
    pub fn gable_roof(&mut self, base: [f32; 3], w: f32, d: f32, h: f32, paint: Paint) {
        let (x0, x1) = (base[0] - w * 0.5, base[0] + w * 0.5);
        let (z0, z1) = (base[2] - d * 0.5, base[2] + d * 0.5);
        let y0 = base[1];
        let y1 = base[1] + h;
        let xm = base[0];
        // Скаты.
        self.quad(
            [x1, y0, z1],
            [x1, y0, z0],
            [xm, y1, z0],
            [xm, y1, z1],
            paint,
        );
        self.quad(
            [x0, y0, z0],
            [x0, y0, z1],
            [xm, y1, z1],
            [xm, y1, z0],
            paint,
        );
        // Фронтоны.
        self.polygon(
            &[[x0, y0, z1], [x1, y0, z1], [xm, y1, z1]],
            &[[0.0, 1.0], [1.0, 1.0], [0.5, 0.0]],
            paint,
        );
        self.polygon(
            &[[x1, y0, z0], [x0, y0, z0], [xm, y1, z0]],
            &[[0.0, 1.0], [1.0, 1.0], [0.5, 0.0]],
            paint,
        );
    }

    /// Четырёхскатная пирамида над прямоугольником `w × d`.
    pub fn pyramid(&mut self, base: [f32; 3], w: f32, d: f32, h: f32, paint: Paint) {
        let (x0, x1) = (base[0] - w * 0.5, base[0] + w * 0.5);
        let (z0, z1) = (base[2] - d * 0.5, base[2] + d * 0.5);
        let y = base[1];
        let tip = [base[0], y + h, base[2]];
        let tri = [[0.0, 1.0], [1.0, 1.0], [0.5, 0.0]];
        self.polygon(&[[x0, y, z1], [x1, y, z1], tip], &tri, paint);
        self.polygon(&[[x1, y, z1], [x1, y, z0], tip], &tri, paint);
        self.polygon(&[[x1, y, z0], [x0, y, z0], tip], &tri, paint);
        self.polygon(&[[x0, y, z0], [x0, y, z1], tip], &tri, paint);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn centroid_dir(mesh: &Mesh, tri: usize) -> ([f32; 3], [f32; 3]) {
        let i = &mesh.indices[tri * 3..tri * 3 + 3];
        let p: Vec<_> = i.iter().map(|&k| mesh.vertices[k as usize].pos).collect();
        let c = [
            (p[0][0] + p[1][0] + p[2][0]) / 3.0,
            (p[0][1] + p[1][1] + p[2][1]) / 3.0,
            (p[0][2] + p[1][2] + p[2][2]) / 3.0,
        ];
        (c, mesh.vertices[i[0] as usize].normal)
    }

    /// Нормаль каждой грани смотрит от центра тела наружу.
    fn assert_outward(mesh: &Mesh, center: [f32; 3]) {
        for t in 0..mesh.triangle_count() {
            let (c, n) = centroid_dir(mesh, t);
            let d = sub(c, center);
            let dot = d[0] * n[0] + d[1] * n[1] + d[2] * n[2];
            assert!(dot > 0.0, "треугольник {t} смотрит внутрь");
        }
    }

    #[test]
    fn primitives_face_outwards() {
        let uvs = MaterialUvs::identity();
        let paint = Paint::plain(Material::Wood);
        let mut b = MeshBuilder::new(&uvs);
        b.cuboid([0.0, 1.0, 0.0], [2.0, 2.0, 2.0], paint);
        assert_outward(&b.finish(), [0.0, 1.0, 0.0]);

        let mut b = MeshBuilder::new(&uvs);
        b.cylinder([0.0, 0.0, 0.0], 1.0, 2.0, 8, paint);
        let m = b.finish();
        assert_outward(&m, [0.0, 1.0, 0.0]);
        assert_eq!(m.triangle_count(), 8 * 2 + 6);

        let mut b = MeshBuilder::new(&uvs);
        b.frustum([0.0, 0.0, 0.0], 1.0, 0.0, 2.0, 6, paint);
        assert_outward(&b.finish(), [0.0, 0.5, 0.0]);

        let mut b = MeshBuilder::new(&uvs);
        b.gable_roof([0.0, 0.0, 0.0], 2.0, 3.0, 1.0, paint);
        assert_outward(&b.finish(), [0.0, 0.3, 0.0]);

        let mut b = MeshBuilder::new(&uvs);
        b.pyramid([0.0, 0.0, 0.0], 2.0, 2.0, 1.0, paint);
        assert_outward(&b.finish(), [0.0, 0.2, 0.0]);
    }

    #[test]
    fn uvs_stay_inside_material_region() {
        let rect = UvRect {
            u0: 0.25,
            v0: 0.5,
            u1: 0.5,
            v1: 0.75,
        };
        let uvs = MaterialUvs {
            rects: [rect; Material::COUNT],
        };
        let mut b = MeshBuilder::new(&uvs);
        b.cylinder(
            [0.0; 3],
            1.0,
            1.0,
            7,
            Paint::team(Material::Cloth, Tint::Secondary),
        );
        let m = b.finish();
        m.validate().unwrap();
        assert!(m.vertices.iter().all(|v| rect.contains(v.uv)));
        assert!(
            m.vertices
                .iter()
                .all(|v| Tint::from_f32(v.tint) == Tint::Secondary)
        );
    }

    #[test]
    fn vertex_is_tightly_packed() {
        assert_eq!(std::mem::size_of::<Vertex>(), 36);
    }
}
