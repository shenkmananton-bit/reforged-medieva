//! Хеш состояния, одинаковый на всех платформах и при каждом запуске.
//!
//! Стандартный `DefaultHasher` для этого не годится: его алгоритм может
//! меняться между версиями Rust, а `HashMap` к тому же сидирует его случайно.
//! Здесь — FNV-1a (64 бит) с явным порядком байт little-endian.

use std::hash::Hasher;

/// Детерминированный хешер. Работает с `#[derive(Hash)]`:
///
/// ```
/// use rm_core::sim::StableHasher;
/// use std::hash::{Hash, Hasher};
///
/// #[derive(Hash)]
/// struct Hp(u16);
///
/// let mut h = StableHasher::new();
/// Hp(100).hash(&mut h);
/// assert_eq!(h.finish(), { let mut h2 = StableHasher::new(); Hp(100).hash(&mut h2); h2.finish() });
/// ```
///
/// `f32`/`f64` не реализуют `Hash` — хешируйте `value.to_bits()`.
#[derive(Clone, Debug)]
pub struct StableHasher {
    state: u64,
}

const FNV_OFFSET: u64 = 0xCBF2_9CE4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01B3;

impl StableHasher {
    pub fn new() -> Self {
        Self { state: FNV_OFFSET }
    }
}

impl Default for StableHasher {
    fn default() -> Self {
        Self::new()
    }
}

impl Hasher for StableHasher {
    #[inline]
    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.state ^= u64::from(b);
            self.state = self.state.wrapping_mul(FNV_PRIME);
        }
    }

    // Числа пишем в little-endian явно, чтобы хеш не зависел от платформы.
    fn write_u16(&mut self, i: u16) {
        self.write(&i.to_le_bytes());
    }
    fn write_u32(&mut self, i: u32) {
        self.write(&i.to_le_bytes());
    }
    fn write_u64(&mut self, i: u64) {
        self.write(&i.to_le_bytes());
    }
    fn write_u128(&mut self, i: u128) {
        self.write(&i.to_le_bytes());
    }
    // usize бывает 32 и 64 бита — всегда расширяем до 64.
    fn write_usize(&mut self, i: usize) {
        self.write_u64(i as u64);
    }
    fn write_i16(&mut self, i: i16) {
        self.write_u16(i as u16);
    }
    fn write_i32(&mut self, i: i32) {
        self.write_u32(i as u32);
    }
    fn write_i64(&mut self, i: i64) {
        self.write_u64(i as u64);
    }
    fn write_i128(&mut self, i: i128) {
        self.write_u128(i as u128);
    }
    fn write_isize(&mut self, i: isize) {
        self.write_u64(i as i64 as u64);
    }

    fn finish(&self) -> u64 {
        self.state
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fnv1a_reference() {
        // Эталон FNV-1a 64 для строки "a".
        let mut h = StableHasher::new();
        h.write(b"a");
        assert_eq!(h.finish(), 0xAF63_DC4C_8601_EC8C);
    }

    #[test]
    fn usize_is_platform_independent() {
        let mut a = StableHasher::new();
        a.write_usize(5);
        let mut b = StableHasher::new();
        b.write_u64(5);
        assert_eq!(a.finish(), b.finish());
    }
}
