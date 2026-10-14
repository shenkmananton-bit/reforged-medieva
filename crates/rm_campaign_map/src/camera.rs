//! Камера карты кампании в стиле Medieval II.
//!
//! Камера смотрит на точку карты сверху под углом. Колесо мыши
//! приближает и отдаляет; чем дальше камера, тем круче она смотрит вниз,
//! пока на максимальном отдалении карта не становится почти плоской
//! «политической» картой. Точку, куда смотрит камера, нельзя увести за край.
//!
//! Координаты мира: x — восток, y — вверх, z — юг (как строки картинки
//! карты). Поэтому «на север» — это −z. Подробно: `docs/campaign-map/coordinates.md`.

use glam::{Mat4, Vec2, Vec3, Vec4};

use crate::map::CampaignMap;

pub const MIN_DISTANCE: f32 = 40.0;
pub const MAX_DISTANCE: f32 = 1100.0;
const FOV_Y: f32 = 0.8;

#[derive(Debug, Clone, PartialEq)]
pub struct MapCamera {
    /// Точка на земле, на которую смотрит камера.
    pub target: Vec3,
    /// Поворот вокруг вертикали, радианы. 0 — камера на юге, смотрит на север.
    pub yaw: f32,
    /// Расстояние от камеры до точки.
    pub distance: f32,
    /// Сторона карты в единицах мира — граница, за которую не уводим точку.
    pub world_size: f32,
}

impl MapCamera {
    /// Камера над центром карты, с большого расстояния.
    pub fn new(world_size: f32) -> Self {
        Self {
            target: Vec3::new(world_size * 0.5, 0.0, world_size * 0.55),
            yaw: 0.0,
            distance: world_size * 0.55,
            world_size,
        }
    }

    /// Наклон над горизонтом: вблизи ~45°, вдали почти сверху (~75°).
    pub fn pitch(&self) -> f32 {
        let t = ((self.distance - MIN_DISTANCE) / (MAX_DISTANCE - MIN_DISTANCE)).clamp(0.0, 1.0);
        0.78 + t.sqrt() * 0.52
    }

    pub fn eye(&self) -> Vec3 {
        let p = self.pitch();
        let horizontal = self.distance * p.cos();
        self.target
            + Vec3::new(
                self.yaw.sin() * horizontal,
                self.distance * p.sin(),
                self.yaw.cos() * horizontal,
            )
    }

    /// «Вперёд» и «вправо» по земле — для движения клавишами.
    pub fn ground_axes(&self) -> (Vec3, Vec3) {
        let forward = Vec3::new(-self.yaw.sin(), 0.0, -self.yaw.cos());
        let right = Vec3::new(self.yaw.cos(), 0.0, -self.yaw.sin());
        (forward, right)
    }

    /// Матрица «мир → экран» для Vulkan (глубина 0..1, ось Y экрана вниз).
    pub fn view_proj(&self, aspect: f32) -> Mat4 {
        let view = Mat4::look_at_rh(self.eye(), self.target, Vec3::Y);
        // Ближняя плоскость растёт с расстоянием: так буфер глубины точнее
        // и далёкие горы не мерцают (та же задача, что у H-05).
        let near = (self.distance * 0.05).max(0.5);
        let mut proj =
            Mat4::perspective_rh(FOV_Y, aspect.max(0.1), near, self.distance * 4.0 + 500.0);
        proj.y_axis.y = -proj.y_axis.y;
        proj * view
    }

    /// Сдвиг по земле: `forward`, `right` — в долях расстояния камеры,
    /// чтобы вдали камера ехала быстрее, а вблизи — точнее.
    pub fn pan(&mut self, forward: f32, right: f32) {
        let (f, r) = self.ground_axes();
        self.target += (f * forward + r * right) * self.distance;
        self.clamp();
    }

    /// Приближение (`steps > 0`) или отдаление колесом мыши.
    pub fn zoom(&mut self, steps: f32) {
        self.distance = (self.distance * 0.87f32.powf(steps)).clamp(MIN_DISTANCE, MAX_DISTANCE);
    }

    pub fn rotate(&mut self, radians: f32) {
        self.yaw = (self.yaw + radians).rem_euclid(std::f32::consts::TAU);
    }

    /// Камера держится над рельефом: точка взгляда поднимается на высоту земли.
    pub fn follow_ground(&mut self, map: &CampaignMap, dt: f32) {
        let ground = map.display_height_at(self.target.x, self.target.z);
        self.target.y += (ground - self.target.y) * (dt * 6.0).min(1.0);
    }

    fn clamp(&mut self) {
        self.target.x = self.target.x.clamp(0.0, self.world_size);
        self.target.z = self.target.z.clamp(0.0, self.world_size);
    }

    /// Луч из камеры через пиксель `cursor` окна размером `size`.
    pub fn ray(&self, cursor: Vec2, size: Vec2) -> (Vec3, Vec3) {
        let inv = self.view_proj(size.x / size.y.max(1.0)).inverse();
        let ndc = Vec2::new(cursor.x / size.x * 2.0 - 1.0, cursor.y / size.y * 2.0 - 1.0);
        let near = inv * Vec4::new(ndc.x, ndc.y, 0.0, 1.0);
        let far = inv * Vec4::new(ndc.x, ndc.y, 1.0, 1.0);
        let near = near.truncate() / near.w;
        let far = far.truncate() / far.w;
        (near, (far - near).normalize())
    }

    /// Точка рельефа под курсором: идём по лучу шагами, пока не уйдём
    /// под землю, потом уточняем половинным делением.
    pub fn pick(&self, map: &CampaignMap, cursor: Vec2, size: Vec2) -> Option<Vec3> {
        let (origin, dir) = self.ray(cursor, size);
        let below = |t: f32| {
            let p = origin + dir * t;
            p.y <= map.display_height_at(p.x, p.z)
        };
        let step = (self.distance * 0.01).max(0.25);
        let max_t = self.distance * 6.0 + 200.0;
        let mut t = 0.0;
        while t < max_t {
            let next = t + step;
            if below(next) {
                let (mut lo, mut hi) = (t, next);
                for _ in 0..16 {
                    let mid = (lo + hi) * 0.5;
                    if below(mid) {
                        hi = mid;
                    } else {
                        lo = mid;
                    }
                }
                let p = origin + dir * hi;
                let inside = (0.0..=self.world_size).contains(&p.x)
                    && (0.0..=self.world_size).contains(&p.z);
                return inside.then_some(p);
            }
            t = next;
        }
        None
    }

    /// Пиксель окна для точки мира (или `None`, если она за камерой).
    pub fn project(&self, p: Vec3, size: Vec2) -> Option<Vec2> {
        let clip = self.view_proj(size.x / size.y.max(1.0)) * p.extend(1.0);
        if clip.w <= 0.01 {
            return None;
        }
        let ndc = clip.truncate() / clip.w;
        Some(Vec2::new(
            (ndc.x + 1.0) * 0.5 * size.x,
            (ndc.y + 1.0) * 0.5 * size.y,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::MapConfig;

    #[test]
    fn center_of_screen_picks_the_target() {
        let map = CampaignMap::generate(&MapConfig {
            size: 128,
            regions: 8,
            ..MapConfig::default()
        });
        let mut cam = MapCamera::new(map.config.world_size());
        cam.distance = 90.0;
        for _ in 0..100 {
            cam.follow_ground(&map, 0.1);
        }
        let size = Vec2::new(1280.0, 720.0);
        let hit = cam
            .pick(&map, size * 0.5, size)
            .expect("луч должен попасть в карту");
        let flat = Vec2::new(hit.x - cam.target.x, hit.z - cam.target.z);
        assert!(flat.length() < 3.0, "{hit:?} vs {:?}", cam.target);
    }

    #[test]
    fn zoom_out_looks_more_from_above() {
        let mut cam = MapCamera::new(1024.0);
        cam.distance = MIN_DISTANCE;
        let low = cam.pitch();
        cam.zoom(-100.0);
        assert_eq!(cam.distance, MAX_DISTANCE);
        assert!(cam.pitch() > low + 0.3);
    }

    #[test]
    fn target_stays_on_the_map() {
        let mut cam = MapCamera::new(1024.0);
        cam.pan(100.0, -100.0);
        assert!(cam.target.x >= 0.0 && cam.target.z >= 0.0);
        assert!(cam.target.x <= 1024.0 && cam.target.z <= 1024.0);
    }

    #[test]
    fn default_camera_looks_north() {
        let cam = MapCamera::new(1024.0);
        assert!(cam.eye().z > cam.target.z, "камера южнее точки взгляда");
        let (fwd, _) = cam.ground_axes();
        assert!((fwd - Vec3::new(0.0, 0.0, -1.0)).length() < 1e-6);
    }
}
