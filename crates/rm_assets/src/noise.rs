//! Детерминированный шум для процедурных текстур.
//!
//! Только целочисленный хеш и простая арифметика `f32`, без `sin`/`cos`
//! из системной библиотеки, поэтому одна и та же картинка получается
//! на Windows и Linux. Шум «бесшовный»: решётка заворачивается по периоду,
//! и текстуру можно класть плиткой без видимых стыков.

/// Перемешивает координаты точки решётки и зерно в одно 32-битное число.
pub fn hash2(x: i32, y: i32, seed: u32) -> u32 {
    let mut h = (x as u32).wrapping_mul(0x8da6_b343)
        ^ (y as u32).wrapping_mul(0xd816_3841)
        ^ seed.wrapping_mul(0xcb1a_b31f);
    h ^= h >> 13;
    h = h.wrapping_mul(0x5bd1_e995);
    h ^= h >> 15;
    h
}

/// Число из хеша в диапазоне `[0, 1)`.
pub fn unit(h: u32) -> f32 {
    (h >> 8) as f32 / (1u32 << 24) as f32
}

fn smooth(t: f32) -> f32 {
    t * t * (3.0 - 2.0 * t)
}

/// Value noise в точке `(x, y)`, решётка с шагом 1 заворачивается по `period`.
/// Результат в `[0, 1)`.
pub fn value(x: f32, y: f32, period: i32, seed: u32) -> f32 {
    let period = period.max(1);
    let xf = x.floor();
    let yf = y.floor();
    let (tx, ty) = (smooth(x - xf), smooth(y - yf));
    let x0 = (xf as i32).rem_euclid(period);
    let y0 = (yf as i32).rem_euclid(period);
    let x1 = (x0 + 1) % period;
    let y1 = (y0 + 1) % period;
    let a = unit(hash2(x0, y0, seed));
    let b = unit(hash2(x1, y0, seed));
    let c = unit(hash2(x0, y1, seed));
    let d = unit(hash2(x1, y1, seed));
    let top = a + (b - a) * tx;
    let bottom = c + (d - c) * tx;
    top + (bottom - top) * ty
}

/// Фрактальный шум: сумма `octaves` слоёв value noise, каждый следующий
/// в два раза мельче и в два раза слабее. `u`, `v` — координаты в `[0, 1)`,
/// `base` — сколько клеток решётки на текстуру в первом слое.
pub fn fbm(u: f32, v: f32, base: i32, octaves: u32, seed: u32) -> f32 {
    let mut sum = 0.0;
    let mut amp = 0.5;
    let mut norm = 0.0;
    let mut period = base.max(1);
    for o in 0..octaves {
        sum += value(
            u * period as f32,
            v * period as f32,
            period,
            seed.wrapping_add(o * 1013),
        ) * amp;
        norm += amp;
        amp *= 0.5;
        period *= 2;
    }
    sum / norm
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noise_is_in_range_and_tiles() {
        for i in 0..64 {
            let u = i as f32 / 64.0;
            let n = fbm(u, 0.3, 4, 4, 7);
            assert!((0.0..1.0).contains(&n));
        }
        // Бесшовность: левый край совпадает с правым.
        let left = fbm(0.0, 0.42, 4, 3, 1);
        let right = fbm(1.0, 0.42, 4, 3, 1);
        assert!((left - right).abs() < 1e-5);
    }
}
