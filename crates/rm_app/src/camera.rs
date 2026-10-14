//! Камера поля боя в стиле Medieval II: точка, вокруг которой камера
//! вращается, угол поворота, наклон и расстояние.
//!
//! Координаты: симуляция живёт на плоскости (x, y), где +y — север.
//! В 3D-мире ось Y смотрит вверх, поэтому точка поля (x, y) рисуется в
//! точке мира (x, высота, −y). Так «вперёд на север» — это −Z, а «вправо» — +X.

use glam::{Mat4, Vec2 as V2, Vec3, Vec4};

/// Перевод точки поля в точку мира.
pub fn to_world(p: rm_battle::Vec2, height: f32) -> Vec3 {
    Vec3::new(p.x, height, -p.y)
}

/// Перевод точки мира в точку поля.
pub fn to_field(p: Vec3) -> rm_battle::Vec2 {
    rm_battle::Vec2::new(p.x, -p.z)
}

#[derive(Debug, Clone, PartialEq)]
pub struct Camera {
    /// Точка на земле, на которую смотрит камера.
    pub target: Vec3,
    /// Поворот вокруг вертикали, радианы. 0 — камера на юге и смотрит на север.
    pub yaw: f32,
    /// Наклон над горизонтом, радианы.
    pub pitch: f32,
    /// Расстояние от камеры до точки, метры.
    pub distance: f32,
}

pub const MIN_DISTANCE: f32 = 8.0;
pub const MAX_DISTANCE: f32 = 420.0;
const FOV_Y: f32 = 0.85;

impl Camera {
    pub fn new(target: Vec3, yaw: f32) -> Self {
        Self {
            target,
            yaw,
            pitch: 0.62,
            distance: 110.0,
        }
    }

    pub fn eye(&self) -> Vec3 {
        let horizontal = self.distance * self.pitch.cos();
        self.target
            + Vec3::new(
                self.yaw.sin() * horizontal,
                self.distance * self.pitch.sin(),
                self.yaw.cos() * horizontal,
            )
    }

    /// Направление «вперёд» по земле и «вправо» — для перемещения камеры клавишами.
    pub fn ground_axes(&self) -> (Vec3, Vec3) {
        let forward = Vec3::new(-self.yaw.sin(), 0.0, -self.yaw.cos());
        let right = Vec3::new(self.yaw.cos(), 0.0, -self.yaw.sin());
        (forward, right)
    }

    /// Матрица «мир → экран» для Vulkan (глубина 0..1, ось Y экрана вниз).
    pub fn view_proj(&self, aspect: f32) -> Mat4 {
        let view = Mat4::look_at_rh(self.eye(), self.target, Vec3::Y);
        let mut proj = Mat4::perspective_rh(FOV_Y, aspect.max(0.1), 0.5, 4000.0);
        proj.y_axis.y = -proj.y_axis.y;
        proj * view
    }

    /// Луч из камеры через пиксель `cursor` окна размером `size`.
    pub fn ray(&self, cursor: V2, size: V2) -> (Vec3, Vec3) {
        let inv = self.view_proj(size.x / size.y.max(1.0)).inverse();
        let ndc = V2::new(cursor.x / size.x * 2.0 - 1.0, cursor.y / size.y * 2.0 - 1.0);
        let near = inv * Vec4::new(ndc.x, ndc.y, 0.0, 1.0);
        let far = inv * Vec4::new(ndc.x, ndc.y, 1.0, 1.0);
        let near = near.truncate() / near.w;
        let far = far.truncate() / far.w;
        (near, (far - near).normalize())
    }

    /// Точка на горизонтальной плоскости высоты `height` под курсором.
    pub fn pick_plane(&self, cursor: V2, size: V2, height: f32) -> Option<Vec3> {
        let (origin, dir) = self.ray(cursor, size);
        if dir.y.abs() < 1e-5 {
            return None;
        }
        let t = (height - origin.y) / dir.y;
        (t > 0.0).then(|| origin + dir * t)
    }

    /// Пиксель окна для точки мира (или `None`, если она за камерой).
    pub fn project(&self, p: Vec3, size: V2) -> Option<V2> {
        let clip = self.view_proj(size.x / size.y.max(1.0)) * p.extend(1.0);
        if clip.w <= 0.01 {
            return None;
        }
        let ndc = clip.truncate() / clip.w;
        Some(V2::new(
            (ndc.x + 1.0) * 0.5 * size.x,
            (ndc.y + 1.0) * 0.5 * size.y,
        ))
    }

    pub fn zoom(&mut self, steps: f32) {
        self.distance = (self.distance * 0.88f32.powf(steps)).clamp(MIN_DISTANCE, MAX_DISTANCE);
        // Чем ближе камера, тем ниже она опускается к земле, как в оригинале.
        let t = (self.distance - MIN_DISTANCE) / (MAX_DISTANCE - MIN_DISTANCE);
        self.pitch = 0.3 + t.sqrt() * 0.75;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn center_of_screen_hits_target() {
        let cam = Camera::new(Vec3::new(10.0, 0.0, -20.0), 0.7);
        let size = V2::new(1280.0, 720.0);
        let hit = cam.pick_plane(size * 0.5, size, 0.0).unwrap();
        assert!((hit - cam.target).length() < 1e-2, "{hit:?}");
        let px = cam.project(cam.target, size).unwrap();
        assert!((px - size * 0.5).length() < 0.5, "{px:?}");
    }

    #[test]
    fn default_camera_looks_north() {
        let cam = Camera::new(Vec3::ZERO, 0.0);
        assert!(cam.eye().z > 0.0, "камера должна стоять на юге");
        let (fwd, right) = cam.ground_axes();
        assert!((fwd - Vec3::new(0.0, 0.0, -1.0)).length() < 1e-6);
        assert!((right - Vec3::X).length() < 1e-6);
        // Север поля — это −Z мира.
        assert_eq!(
            to_world(rm_battle::Vec2::NORTH, 0.0),
            Vec3::new(0.0, 0.0, -1.0)
        );
    }
}
