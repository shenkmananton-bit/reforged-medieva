//! Двумерный вектор для симуляции.
//!
//! Битва считается на плоскости (хак H-07: высоты нет, поле ровное).
//! Используются только `+ - * /` и `sqrt` — они одинаково округляются
//! на всех платформах, поэтому симуляция детерминирована
//! (`docs/adr/determinism.md`). Тригонометрия не нужна: направления
//! хранятся как единичные векторы, а не как углы.

use std::hash::{Hash, Hasher};
use std::ops::{Add, AddAssign, Mul, Neg, Sub, SubAssign};

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Vec2 {
    pub x: f32,
    pub y: f32,
}

impl Vec2 {
    pub const ZERO: Self = Self { x: 0.0, y: 0.0 };
    /// «Север»: направление от первой армии ко второй в стандартной расстановке.
    pub const NORTH: Self = Self { x: 0.0, y: 1.0 };

    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    pub fn dot(self, o: Self) -> f32 {
        self.x * o.x + self.y * o.y
    }

    pub fn length_sq(self) -> f32 {
        self.dot(self)
    }

    pub fn length(self) -> f32 {
        self.length_sq().sqrt()
    }

    pub fn distance(self, o: Self) -> f32 {
        (self - o).length()
    }

    pub fn distance_sq(self, o: Self) -> f32 {
        (self - o).length_sq()
    }

    /// Единичный вектор того же направления; для нулевого — `fallback`.
    pub fn normalize_or(self, fallback: Self) -> Self {
        let len = self.length();
        if len > 1e-6 {
            self * (1.0 / len)
        } else {
            fallback
        }
    }

    /// Вектор «направо» от направления взгляда: для севера это восток.
    pub fn right(self) -> Self {
        Self::new(self.y, -self.x)
    }

    /// Ограничивает длину вектора.
    pub fn clamp_length(self, max: f32) -> Self {
        let len_sq = self.length_sq();
        if len_sq > max * max {
            self * (max / len_sq.sqrt())
        } else {
            self
        }
    }

    pub fn lerp(self, o: Self, t: f32) -> Self {
        self + (o - self) * t
    }
}

impl Add for Vec2 {
    type Output = Self;
    fn add(self, o: Self) -> Self {
        Self::new(self.x + o.x, self.y + o.y)
    }
}

impl Sub for Vec2 {
    type Output = Self;
    fn sub(self, o: Self) -> Self {
        Self::new(self.x - o.x, self.y - o.y)
    }
}

impl Mul<f32> for Vec2 {
    type Output = Self;
    fn mul(self, k: f32) -> Self {
        Self::new(self.x * k, self.y * k)
    }
}

impl Neg for Vec2 {
    type Output = Self;
    fn neg(self) -> Self {
        Self::new(-self.x, -self.y)
    }
}

impl AddAssign for Vec2 {
    fn add_assign(&mut self, o: Self) {
        *self = *self + o;
    }
}

impl SubAssign for Vec2 {
    fn sub_assign(&mut self, o: Self) {
        *self = *self - o;
    }
}

/// Хеш по битам чисел — для контрольных точек реплея.
impl Hash for Vec2 {
    fn hash<H: Hasher>(&self, h: &mut H) {
        self.x.to_bits().hash(h);
        self.y.to_bits().hash(h);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn right_of_north_is_east() {
        assert_eq!(Vec2::NORTH.right(), Vec2::new(1.0, 0.0));
    }

    #[test]
    fn normalize_and_clamp() {
        let v = Vec2::new(3.0, 4.0);
        assert!((v.normalize_or(Vec2::ZERO).length() - 1.0).abs() < 1e-6);
        assert_eq!(Vec2::ZERO.normalize_or(Vec2::NORTH), Vec2::NORTH);
        assert!((v.clamp_length(1.0).length() - 1.0).abs() < 1e-6);
        assert_eq!(v.clamp_length(10.0), v);
    }
}
