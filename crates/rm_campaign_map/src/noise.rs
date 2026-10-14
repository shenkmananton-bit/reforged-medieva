//! Шум для процедурной карты: value noise и его сумма по октавам (fBm).
//!
//! Шум считается от целочисленного хеша координат, без таблиц и без
//! внешних библиотек. Все операции — сложение, умножение и `floor`, они
//! одинаковы до бита на всех платформах, поэтому одна и та же карта
//! получается из одного зерна на Windows и на Linux.

/// Хеш точки решётки в число 0..1.
fn lattice(seed: u32, x: i32, y: i32) -> f32 {
    let mut h = seed ^ (x as u32).wrapping_mul(0x27d4_eb2d) ^ (y as u32).wrapping_mul(0x1656_67b1);
    h ^= h >> 15;
    h = h.wrapping_mul(0x85eb_ca6b);
    h ^= h >> 13;
    h = h.wrapping_mul(0xc2b2_ae35);
    h ^= h >> 16;
    (h >> 8) as f32 / (1u32 << 24) as f32
}

/// Плавная кривая 3t² − 2t³: убирает «ступеньки» на границах клеток.
fn smooth(t: f32) -> f32 {
    t * t * (3.0 - 2.0 * t)
}

/// Value noise: случайные значения в узлах решётки, плавно смешанные между ними.
/// Результат 0..1.
pub fn value(seed: u32, x: f32, y: f32) -> f32 {
    let (fx, fy) = (x.floor(), y.floor());
    let (ix, iy) = (fx as i32, fy as i32);
    let (tx, ty) = (smooth(x - fx), smooth(y - fy));
    let a = lattice(seed, ix, iy);
    let b = lattice(seed, ix + 1, iy);
    let c = lattice(seed, ix, iy + 1);
    let d = lattice(seed, ix + 1, iy + 1);
    let top = a + (b - a) * tx;
    let bottom = c + (d - c) * tx;
    top + (bottom - top) * ty
}

/// Сумма `octaves` слоёв шума: каждый следующий вдвое мельче и вдвое слабее.
/// Крупные слои дают материки, мелкие — изрезанный берег. Результат 0..1.
pub fn fbm(seed: u32, x: f32, y: f32, octaves: u32) -> f32 {
    let (mut sum, mut amp, mut freq, mut norm) = (0.0, 1.0, 1.0, 0.0);
    for o in 0..octaves {
        sum += value(seed.wrapping_add(o * 101), x * freq, y * freq) * amp;
        norm += amp;
        amp *= 0.5;
        freq *= 2.0;
    }
    sum / norm
}

/// «Хребтовый» шум: 1 − |2n − 1| даёт острые гребни там, где обычный шум
/// проходит через середину. Из него получаются горные цепи. Результат 0..1.
pub fn ridged(seed: u32, x: f32, y: f32, octaves: u32) -> f32 {
    let (mut sum, mut amp, mut freq, mut norm) = (0.0, 1.0, 1.0, 0.0);
    for o in 0..octaves {
        let n = value(seed.wrapping_add(o * 211), x * freq, y * freq);
        let r = 1.0 - (2.0 * n - 1.0).abs();
        sum += r * r * amp;
        norm += amp;
        amp *= 0.5;
        freq *= 2.0;
    }
    sum / norm
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noise_is_in_range_and_repeatable() {
        for i in 0..500 {
            let (x, y) = (i as f32 * 0.37, i as f32 * 0.11 - 20.0);
            for v in [value(7, x, y), fbm(7, x, y, 5), ridged(7, x, y, 4)] {
                assert!((0.0..=1.0).contains(&v), "{v}");
            }
            assert_eq!(fbm(7, x, y, 5), fbm(7, x, y, 5));
        }
    }

    #[test]
    fn noise_is_continuous() {
        // Соседние точки дают близкие значения — нет разрывов.
        let a = value(3, 10.5, 4.25);
        let b = value(3, 10.501, 4.25);
        assert!((a - b).abs() < 0.01);
    }

    #[test]
    fn different_seeds_differ() {
        assert_ne!(value(1, 3.3, 4.4), value(2, 3.3, 4.4));
    }
}
