//! Детерминированная математика для симуляции.
//!
//! `f32::sin`, `cos`, `atan2` из стандартной библиотеки вызывают системную
//! libm, и последний бит результата может отличаться между Windows, Linux и
//! разными процессорами. Для реплеев это недопустимо (`docs/adr/determinism.md`).
//!
//! Здесь те же функции посчитаны только через `+ - * /` и `round` — операции,
//! которые по стандарту IEEE 754 округляются одинаково везде. Внутри считаем в
//! `f64`, результат отдаём в `f32`: точность ~1e-7, этого с запасом хватает игре.

use std::f64::consts::{FRAC_PI_2, FRAC_PI_6, PI};

/// Синус и косинус одновременно (дешевле, чем по отдельности).
pub fn sin_cos(x: f32) -> (f32, f32) {
    let x = f64::from(x);
    // Приводим угол к r ∈ [-π/4, π/4]: x = k·(π/2) + r.
    let k = (x / FRAC_PI_2).round();
    let r = x - k * FRAC_PI_2;
    let (s, c) = (sin_poly(r), cos_poly(r));
    // Четверть круга определяет, какая из функций и с каким знаком.
    let (sin, cos) = match (k as i64).rem_euclid(4) {
        0 => (s, c),
        1 => (c, -s),
        2 => (-s, -c),
        _ => (-c, s),
    };
    (sin as f32, cos as f32)
}

pub fn sin(x: f32) -> f32 {
    sin_cos(x).0
}

pub fn cos(x: f32) -> f32 {
    sin_cos(x).1
}

/// Ряд Тейлора для sin на [-π/4, π/4]; ошибка < 1e-11.
#[inline]
fn sin_poly(r: f64) -> f64 {
    let r2 = r * r;
    r * (1.0
        + r2 * (-1.0 / 6.0
            + r2 * (1.0 / 120.0
                + r2 * (-1.0 / 5040.0 + r2 * (1.0 / 362_880.0 + r2 * (-1.0 / 39_916_800.0))))))
}

/// Ряд Тейлора для cos на [-π/4, π/4]; ошибка < 1e-12.
#[inline]
fn cos_poly(r: f64) -> f64 {
    let r2 = r * r;
    1.0 + r2
        * (-0.5
            + r2 * (1.0 / 24.0
                + r2 * (-1.0 / 720.0
                    + r2 * (1.0 / 40_320.0
                        + r2 * (-1.0 / 3_628_800.0 + r2 * (1.0 / 479_001_600.0))))))
}

/// Арктангенс на всём диапазоне.
pub fn atan(x: f32) -> f32 {
    atan_f64(f64::from(x)) as f32
}

fn atan_f64(x: f64) -> f64 {
    let (sign, x) = if x < 0.0 { (-1.0, -x) } else { (1.0, x) };
    // atan(x) = π/2 − atan(1/x) для x > 1.
    let (base, x) = if x > 1.0 {
        (FRAC_PI_2, -1.0 / x)
    } else {
        (0.0, x)
    };
    // Сдвиг на π/6, чтобы аргумент ряда стал маленьким (|t| ≤ tan(π/12)).
    const TAN_PI_12: f64 = 0.267_949_192_431_122_7;
    const SQRT_3: f64 = 1.732_050_807_568_877_2;
    let ax = if x < 0.0 { -x } else { x };
    let (shift, t) = if ax > TAN_PI_12 {
        let t = (ax * SQRT_3 - 1.0) / (ax + SQRT_3);
        (FRAC_PI_6, t)
    } else {
        (0.0, ax)
    };
    let t2 = t * t;
    // Ряд atan(t) = t − t³/3 + t⁵/5 − … до t¹⁷; ошибка < 1e-11 при |t| ≤ 0.268.
    let series = t
        * (1.0
            + t2 * (-1.0 / 3.0
                + t2 * (1.0 / 5.0
                    + t2 * (-1.0 / 7.0
                        + t2 * (1.0 / 9.0
                            + t2 * (-1.0 / 11.0
                                + t2 * (1.0 / 13.0 + t2 * (-1.0 / 15.0 + t2 * (1.0 / 17.0)))))))));
    let small = shift + series;
    let small = if x < 0.0 { -small } else { small };
    sign * (base + small)
}

/// Угол вектора `(x, y)` в радианах, `(-π, π]`. Аналог `f32::atan2(y, x)`.
pub fn atan2(y: f32, x: f32) -> f32 {
    let (y, x) = (f64::from(y), f64::from(x));
    let r = if x > 0.0 {
        atan_f64(y / x)
    } else if x < 0.0 {
        if y >= 0.0 {
            atan_f64(y / x) + PI
        } else {
            atan_f64(y / x) - PI
        }
    } else if y > 0.0 {
        FRAC_PI_2
    } else if y < 0.0 {
        -FRAC_PI_2
    } else {
        0.0
    };
    r as f32
}

/// Квадратный корень. `sqrt` по IEEE 754 детерминирован, обёртка — чтобы
/// код симуляции брал всю математику из одного модуля.
#[inline]
pub fn sqrt(x: f32) -> f32 {
    x.sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sin_cos_match_std() {
        let mut x = -100.0f32;
        while x < 100.0 {
            let (s, c) = sin_cos(x);
            assert!((s - x.sin()).abs() < 2e-6, "sin({x}) = {s} vs {}", x.sin());
            assert!((c - x.cos()).abs() < 2e-6, "cos({x}) = {c} vs {}", x.cos());
            x += 0.0137;
        }
    }

    #[test]
    fn atan2_matches_std() {
        let vals = [
            -1000.0f32, -3.0, -1.0, -0.5, -0.1, 0.0, 0.1, 0.27, 0.5, 1.0, 2.0, 3.7, 1000.0,
        ];
        for &y in &vals {
            for &x in &vals {
                let a = atan2(y, x);
                let b = y.atan2(x);
                assert!((a - b).abs() < 2e-6, "atan2({y}, {x}) = {a} vs {b}");
            }
        }
    }

    #[test]
    fn exact_points() {
        assert_eq!(sin(0.0), 0.0);
        assert_eq!(cos(0.0), 1.0);
        assert_eq!(atan(1.0), std::f32::consts::FRAC_PI_4);
    }
}
