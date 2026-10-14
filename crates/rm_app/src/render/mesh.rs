//! Процедурные низкополигональные модели: солдаты, всадники, стрелы, знамёна.
//!
//! Настоящих моделей пока нет (оригинальные ассеты M2 в репозиторий не
//! кладём), поэтому фигурки собираются из коробок прямо в коде. Модель
//! стоит на земле (y = 0), смотрит вдоль +z, x — вправо. Рост пехотинца
//! около 1.8 м. Части, окрашенные в цвет фракции, помечены в альфа-канале.

/// Вершина модели. Раскладка совпадает с `mesh.vert`.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vertex {
    pub pos: [f32; 3],
    pub normal: [f32; 3],
    /// rgb — цвет; a: 0 — как есть, 1 — умножить на основной цвет фракции, 2 — на второй.
    pub color: [f32; 4],
}

/// Данные одного экземпляра модели. Раскладка совпадает с `mesh.vert`.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Instance {
    /// xyz — позиция в мире, w — масштаб.
    pub pos_scale: [f32; 4],
    /// xy — направление взгляда на плоскости (x, z), z — фаза шага, w — удар 0..1.
    pub dir_anim: [f32; 4],
    /// Основной цвет; альфа — прозрачность (для полупрозрачных меток).
    pub color1: [f32; 4],
    pub color2: [f32; 4],
}

/// Какие модели есть. Порядок задаёт порядок отрисовки.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MeshId {
    Levy = 0,
    Spearman = 1,
    Crossbowman = 2,
    Horseman = 3,
    Arrow = 4,
    Banner = 5,
    /// Плоский круг на земле: тени, метки выделения, точки назначения.
    Disc = 6,
}

pub const MESH_COUNT: usize = 7;

/// Где модель лежит в общем буфере вершин.
#[derive(Debug, Clone, Copy, Default)]
pub struct MeshRange {
    pub first: u32,
    pub count: u32,
}

const SKIN: [f32; 4] = [0.85, 0.62, 0.48, 0.0];
const DARK: [f32; 4] = [0.22, 0.18, 0.14, 0.0];
const STEEL: [f32; 4] = [0.62, 0.64, 0.68, 0.0];
const WOOD: [f32; 4] = [0.45, 0.30, 0.16, 0.0];
const TEAM1: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
const TEAM2: [f32; 4] = [1.0, 1.0, 1.0, 2.0];
const HORSE: [f32; 4] = [0.36, 0.24, 0.15, 0.0];

/// Строит все модели в один массив вершин и возвращает диапазоны.
pub fn build_all() -> (Vec<Vertex>, [MeshRange; MESH_COUNT]) {
    let mut verts = Vec::new();
    let mut ranges = [MeshRange::default(); MESH_COUNT];
    let builders: [(MeshId, BuildFn); MESH_COUNT] = [
        (MeshId::Levy, levy),
        (MeshId::Spearman, spearman),
        (MeshId::Crossbowman, crossbowman),
        (MeshId::Horseman, horseman),
        (MeshId::Arrow, arrow),
        (MeshId::Banner, banner),
        (MeshId::Disc, disc),
    ];
    for (id, build) in builders {
        let first = verts.len() as u32;
        let mut b = Builder { verts: &mut verts };
        build(&mut b);
        ranges[id as usize] = MeshRange {
            first,
            count: verts.len() as u32 - first,
        };
    }
    (verts, ranges)
}

/// Функция, которая дописывает модель в общий массив вершин.
type BuildFn = fn(&mut Builder);

struct Builder<'a> {
    verts: &'a mut Vec<Vertex>,
}

impl Builder<'_> {
    /// Коробка с центром `c` и размерами `s`, плоские нормали (low-poly вид).
    fn cuboid(&mut self, c: [f32; 3], s: [f32; 3], color: [f32; 4]) {
        let h = [s[0] * 0.5, s[1] * 0.5, s[2] * 0.5];
        // Шесть граней: нормаль и два вектора вдоль грани.
        let faces: [([f32; 3], [f32; 3], [f32; 3]); 6] = [
            ([1.0, 0.0, 0.0], [0.0, 0.0, -1.0], [0.0, 1.0, 0.0]),
            ([-1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, 1.0, 0.0]),
            ([0.0, 1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, -1.0]),
            ([0.0, -1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
            ([0.0, 0.0, 1.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
            ([0.0, 0.0, -1.0], [-1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
        ];
        for (n, u, v) in faces {
            let corner = |a: f32, b: f32| {
                [
                    c[0] + (n[0] + u[0] * a + v[0] * b) * h[0],
                    c[1] + (n[1] + u[1] * a + v[1] * b) * h[1],
                    c[2] + (n[2] + u[2] * a + v[2] * b) * h[2],
                ]
            };
            let quad = [
                corner(-1.0, -1.0),
                corner(1.0, -1.0),
                corner(1.0, 1.0),
                corner(-1.0, -1.0),
                corner(1.0, 1.0),
                corner(-1.0, 1.0),
            ];
            for pos in quad {
                self.verts.push(Vertex {
                    pos,
                    normal: n,
                    color,
                });
            }
        }
    }

    /// Пехотинец без оружия: ноги, туловище, руки, голова, шлем.
    fn body(&mut self, helmet: [f32; 4]) {
        self.cuboid([-0.11, 0.4, 0.0], [0.15, 0.8, 0.18], DARK);
        self.cuboid([0.11, 0.4, 0.0], [0.15, 0.8, 0.18], DARK);
        self.cuboid([0.0, 1.12, 0.0], [0.46, 0.66, 0.28], TEAM1);
        self.cuboid([0.0, 0.86, 0.0], [0.48, 0.1, 0.3], TEAM2);
        self.cuboid([-0.3, 1.12, 0.06], [0.12, 0.56, 0.14], TEAM1);
        self.cuboid([0.3, 1.12, 0.06], [0.12, 0.56, 0.14], TEAM1);
        self.cuboid([0.0, 1.58, 0.0], [0.22, 0.24, 0.24], SKIN);
        self.cuboid([0.0, 1.73, 0.0], [0.26, 0.1, 0.28], helmet);
    }
}

fn levy(b: &mut Builder) {
    b.body(WOOD);
    // Дубина в правой руке.
    b.cuboid([0.32, 1.25, 0.35], [0.08, 0.08, 0.7], WOOD);
}

fn spearman(b: &mut Builder) {
    b.body(STEEL);
    // Длинное копьё вперёд и круглый (здесь — квадратный) щит в цвет фракции.
    b.cuboid([0.32, 1.25, 0.6], [0.06, 0.06, 2.6], WOOD);
    b.cuboid([0.32, 1.25, 1.95], [0.1, 0.1, 0.2], STEEL);
    b.cuboid([-0.36, 1.1, 0.2], [0.06, 0.6, 0.55], TEAM2);
}

fn crossbowman(b: &mut Builder) {
    b.body(STEEL);
    b.cuboid([0.0, 1.25, 0.4], [0.1, 0.1, 0.6], WOOD);
    b.cuboid([0.0, 1.25, 0.62], [0.6, 0.06, 0.06], STEEL);
    // Колчан за спиной.
    b.cuboid([0.18, 1.2, -0.2], [0.1, 0.4, 0.1], WOOD);
}

fn horseman(b: &mut Builder) {
    // Конь: ноги, корпус, шея, голова, попона в цвет фракции.
    for (x, z) in [(-0.2, 0.7), (0.2, 0.7), (-0.2, -0.7), (0.2, -0.7)] {
        b.cuboid([x, 0.45, z], [0.14, 0.9, 0.14], HORSE);
    }
    b.cuboid([0.0, 1.1, 0.0], [0.5, 0.5, 1.7], HORSE);
    b.cuboid([0.0, 1.0, 0.0], [0.56, 0.3, 1.2], TEAM2);
    b.cuboid([0.0, 1.5, 0.95], [0.3, 0.6, 0.3], HORSE);
    b.cuboid([0.0, 1.75, 1.2], [0.24, 0.24, 0.5], HORSE);
    // Всадник.
    b.cuboid([0.0, 1.75, -0.05], [0.42, 0.6, 0.28], TEAM1);
    b.cuboid([-0.28, 1.75, 0.05], [0.12, 0.5, 0.14], TEAM1);
    b.cuboid([0.28, 1.75, 0.05], [0.12, 0.5, 0.14], TEAM1);
    b.cuboid([0.0, 2.17, -0.05], [0.22, 0.24, 0.24], SKIN);
    b.cuboid([0.0, 2.32, -0.05], [0.26, 0.1, 0.28], STEEL);
    // Лук.
    b.cuboid([-0.4, 1.8, 0.2], [0.05, 0.9, 0.06], WOOD);
}

fn arrow(b: &mut Builder) {
    b.cuboid([0.0, 0.0, 0.0], [0.04, 0.04, 0.9], WOOD);
    b.cuboid([0.0, 0.0, 0.45], [0.06, 0.06, 0.1], STEEL);
}

fn banner(b: &mut Builder) {
    b.cuboid([0.0, 2.0, 0.0], [0.08, 4.0, 0.08], WOOD);
    b.cuboid([0.0, 3.4, -0.75], [0.05, 1.1, 1.4], TEAM1);
    b.cuboid([0.0, 3.4, -0.75], [0.06, 0.3, 1.42], TEAM2);
}

fn disc(b: &mut Builder) {
    // Восьмиугольник радиусом 1, лежит на земле. Цвет берётся целиком из экземпляра.
    const N: usize = 8;
    let white = [1.0, 1.0, 1.0, 1.0];
    for i in 0..N {
        let a0 = i as f32 / N as f32 * std::f32::consts::TAU;
        let a1 = (i + 1) as f32 / N as f32 * std::f32::consts::TAU;
        for pos in [
            [0.0, 0.0, 0.0],
            [a1.cos(), 0.0, a1.sin()],
            [a0.cos(), 0.0, a0.sin()],
        ] {
            b.verts.push(Vertex {
                pos,
                normal: [0.0, 1.0, 0.0],
                color: white,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_meshes_are_built_back_to_back() {
        let (verts, ranges) = build_all();
        let mut next = 0;
        for r in ranges {
            assert_eq!(r.first, next);
            assert!(r.count > 0 && r.count % 3 == 0);
            next += r.count;
        }
        assert_eq!(next as usize, verts.len());
    }

    #[test]
    fn vertex_layouts_match_shaders() {
        assert_eq!(std::mem::size_of::<Vertex>(), 40);
        assert_eq!(std::mem::size_of::<Instance>(), 64);
    }
}
