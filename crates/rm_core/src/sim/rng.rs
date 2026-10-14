//! Детерминированный генератор случайных чисел.
//!
//! Два способа получить случайность:
//!
//! 1. [`random_u64`] — чистая функция от ключа. Никакого состояния: одно и
//!    то же `(seed, поток, шаг, сущность, вызов)` всегда даёт одно число.
//! 2. [`Rng`] — обычный последовательный генератор (xoshiro256\*\*), который
//!    удобно создавать из того же ключа через [`Rng::for_entity`]. Номер
//!    вызова тогда — просто позиция в его последовательности.
//!
//! Оба варианта используют только целочисленные операции, поэтому результат
//! одинаков на любой платформе и при любом числе потоков.

/// Назначение случайности. Разные системы берут разные потоки, чтобы новый
/// бросок кубика в одной системе не сдвигал числа в другой.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RngStream(pub u64);

impl RngStream {
    pub const COMBAT: Self = Self(1);
    pub const MORALE: Self = Self(2);
    pub const BATTLE_AI: Self = Self(3);
    pub const CAMPAIGN_AI: Self = Self(4);
    pub const CAMPAIGN_EVENTS: Self = Self(5);
    pub const PROJECTILES: Self = Self(6);
}

/// Финализатор SplitMix64: хорошо перемешивает биты 64-битного числа.
#[inline]
const fn mix64(mut z: u64) -> u64 {
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

const GOLDEN: u64 = 0x9E37_79B9_7F4A_7C15;

/// Последовательно «вмешивает» части ключа.
#[inline]
const fn combine(h: u64, v: u64) -> u64 {
    mix64(h.wrapping_add(GOLDEN) ^ v)
}

/// Случайное число как чистая функция ключа.
///
/// `entity` — обычно `Entity::to_bits()`, `call` — номер броска внутри шага
/// для этой сущности (0, 1, 2…).
#[inline]
pub const fn random_u64(seed: u64, stream: RngStream, tick: u64, entity: u64, call: u64) -> u64 {
    let mut h = mix64(seed ^ GOLDEN);
    h = combine(h, stream.0);
    h = combine(h, tick);
    h = combine(h, entity);
    combine(h, call)
}

/// Последовательный генератор xoshiro256\*\*.
///
/// Состояние — 32 байта; его можно сохранить ([`Rng::state`]) и восстановить
/// ([`Rng::from_state`]) — например, в сохранении игры.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rng {
    s: [u64; 4],
}

impl Rng {
    /// Генератор из одного числа (seed).
    pub fn new(seed: u64) -> Self {
        let mut sm = seed;
        let mut next = || {
            sm = sm.wrapping_add(GOLDEN);
            mix64(sm)
        };
        let s = [next(), next(), next(), next()];
        Self::from_state(s)
    }

    /// Генератор для конкретной сущности на конкретном шаге. Не зависит от
    /// порядка обхода и номера потока — его можно создавать внутри
    /// `par_for_each_mut`.
    pub fn for_entity(seed: u64, stream: RngStream, tick: u64, entity: u64) -> Self {
        Self::new(random_u64(seed, stream, tick, entity, u64::MAX))
    }

    /// Генератор для системы целиком (без привязки к сущности).
    pub fn for_stream(seed: u64, stream: RngStream, tick: u64) -> Self {
        Self::for_entity(seed, stream, tick, u64::MAX)
    }

    pub fn from_state(s: [u64; 4]) -> Self {
        // Нулевое состояние у xoshiro «залипает» — подменяем.
        if s == [0; 4] {
            Self::new(0)
        } else {
            Self { s }
        }
    }

    pub fn state(&self) -> [u64; 4] {
        self.s
    }

    #[inline]
    pub fn next_u64(&mut self) -> u64 {
        let result = self.s[1].wrapping_mul(5).rotate_left(7).wrapping_mul(9);
        let t = self.s[1] << 17;
        self.s[2] ^= self.s[0];
        self.s[3] ^= self.s[1];
        self.s[1] ^= self.s[2];
        self.s[0] ^= self.s[3];
        self.s[2] ^= t;
        self.s[3] = self.s[3].rotate_left(45);
        result
    }

    #[inline]
    pub fn next_u32(&mut self) -> u32 {
        (self.next_u64() >> 32) as u32
    }

    /// Равномерно в `0..n` без перекоса (метод Лемира). `n` должно быть > 0.
    #[inline]
    pub fn below(&mut self, n: u32) -> u32 {
        assert!(n > 0, "below(0)");
        let mut m = u64::from(self.next_u32()) * u64::from(n);
        if (m as u32) < n {
            let threshold = n.wrapping_neg() % n;
            while (m as u32) < threshold {
                m = u64::from(self.next_u32()) * u64::from(n);
            }
        }
        (m >> 32) as u32
    }

    /// Равномерно в `lo..=hi`.
    pub fn range_i32(&mut self, lo: i32, hi: i32) -> i32 {
        assert!(lo <= hi, "range_i32: lo > hi");
        let span = (i64::from(hi) - i64::from(lo) + 1) as u64;
        if span > u64::from(u32::MAX) {
            return self.next_u32() as i32;
        }
        (i64::from(lo) + i64::from(self.below(span as u32))) as i32
    }

    /// `true` с вероятностью `numerator / denominator` (целочисленно, без float).
    /// Например, шанс попадания 35% — `chance(35, 100)`.
    pub fn chance(&mut self, numerator: u32, denominator: u32) -> bool {
        self.below(denominator) < numerator
    }

    /// `f32` в `[0, 1)`. Детерминирован (одно целочисленное → float преобразование).
    #[inline]
    pub fn next_f32(&mut self) -> f32 {
        (self.next_u32() >> 8) as f32 * (1.0 / (1u32 << 24) as f32)
    }

    /// Перемешивание Фишера–Йейтса.
    pub fn shuffle<T>(&mut self, items: &mut [T]) {
        for i in (1..items.len()).rev() {
            let j = self.below((i + 1) as u32) as usize;
            items.swap(i, j);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xoshiro_reference_vector() {
        // Эталон из reference-реализации xoshiro256** с состоянием [1, 2, 3, 4].
        let mut rng = Rng::from_state([1, 2, 3, 4]);
        let got: Vec<u64> = (0..4).map(|_| rng.next_u64()).collect();
        assert_eq!(got, [11520, 0, 1509978240, 1215971899390074240]);
    }

    #[test]
    fn same_seed_same_sequence() {
        let a: Vec<u64> = {
            let mut r = Rng::new(42);
            (0..100).map(|_| r.next_u64()).collect()
        };
        let mut r = Rng::new(42);
        assert!(a.iter().all(|&x| x == r.next_u64()));
        assert_ne!(Rng::new(42).next_u64(), Rng::new(43).next_u64());
    }

    #[test]
    fn keyed_random_is_stable_and_distinct() {
        // Значения зафиксированы: если этот тест упал, алгоритм поменялся и
        // все старые реплеи разойдутся. Менять только осознанно.
        let base = random_u64(1, RngStream::COMBAT, 2, 3, 4);
        assert_eq!(base, 0xe5ae_5d9d_dc92_b6c2);
        assert_eq!(Rng::new(42).next_u64(), 0x1578_0b2e_0c2e_c716);
        assert_ne!(base, random_u64(1, RngStream::MORALE, 2, 3, 4));
        assert_ne!(base, random_u64(1, RngStream::COMBAT, 3, 3, 4));
        assert_ne!(base, random_u64(1, RngStream::COMBAT, 2, 4, 4));
        assert_ne!(base, random_u64(1, RngStream::COMBAT, 2, 3, 5));
    }

    #[test]
    fn below_is_in_range_and_roughly_uniform() {
        let mut rng = Rng::new(7);
        let mut counts = [0u32; 6];
        for _ in 0..60_000 {
            counts[rng.below(6) as usize] += 1;
        }
        assert!(
            counts.iter().all(|&c| (9_000..11_000).contains(&c)),
            "{counts:?}"
        );
        for _ in 0..1000 {
            let v = rng.range_i32(-3, 3);
            assert!((-3..=3).contains(&v));
            let f = rng.next_f32();
            assert!((0.0..1.0).contains(&f));
        }
        let _ = rng.range_i32(i32::MIN, i32::MAX);
    }

    #[test]
    fn chance_extremes() {
        let mut rng = Rng::new(1);
        assert!((0..100).all(|_| rng.chance(100, 100)));
        assert!((0..100).all(|_| !rng.chance(0, 100)));
    }
}
