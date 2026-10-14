//! Фиксированный шаг симуляции.
//!
//! Рендер рисует кадры с любой частотой (30, 60, 144 Гц), а битва всегда
//! считается шагами ровно по 50 мс (20 Гц). Реальное время кадра копится в
//! «аккумуляторе»; когда накопилось на шаг — выполняем шаг. Остаток даёт
//! [`FixedTimestep::alpha`] для плавной интерполяции картинки между шагами.
//!
//! Время хранится в целых наносекундах, поэтому за часы игры не набегает
//! ошибка округления, как было бы с `f32`.

use std::time::Duration;

#[derive(Clone, Debug)]
pub struct FixedTimestep {
    step_ns: u64,
    accumulator_ns: u64,
    max_steps_per_update: u32,
    tick: u64,
}

impl FixedTimestep {
    /// Частота боевой симуляции из `docs/adr/determinism.md`.
    pub const BATTLE_HZ: u32 = 20;

    /// Шаг с частотой `hz`. Не более 5 шагов за один вызов `advance`
    /// (защита от «спирали смерти», см. [`Self::with_max_steps`]).
    pub fn new(hz: u32) -> Self {
        assert!(hz > 0, "частота должна быть > 0");
        Self {
            step_ns: 1_000_000_000 / u64::from(hz),
            accumulator_ns: 0,
            max_steps_per_update: 5,
            tick: 0,
        }
    }

    /// Шаг битвы: 20 Гц.
    pub fn battle() -> Self {
        Self::new(Self::BATTLE_HZ)
    }

    /// Сколько шагов максимум выполнять за один кадр. Если машина не успевает,
    /// лишнее время отбрасывается: игра замедляется, но не зависает.
    pub fn with_max_steps(mut self, max_steps: u32) -> Self {
        self.max_steps_per_update = max_steps.max(1);
        self
    }

    /// Добавляет прошедшее реальное время и возвращает, сколько шагов
    /// симуляции нужно выполнить сейчас:
    ///
    /// ```
    /// # use rm_core::FixedTimestep;
    /// # use std::time::Duration;
    /// let mut time = FixedTimestep::battle();
    /// for _ in 0..time.advance(Duration::from_millis(120)) {
    ///     // world.step(time.step_seconds());
    /// }
    /// assert_eq!(time.tick(), 2);
    /// ```
    pub fn advance(&mut self, real_dt: Duration) -> u32 {
        let dt_ns = u64::try_from(real_dt.as_nanos()).unwrap_or(u64::MAX);
        self.accumulator_ns = self.accumulator_ns.saturating_add(dt_ns);
        let wanted = self.accumulator_ns / self.step_ns;
        let steps = wanted.min(u64::from(self.max_steps_per_update));
        if wanted > steps {
            // Не успеваем: оставляем только дробную часть шага.
            self.accumulator_ns %= self.step_ns;
        } else {
            self.accumulator_ns -= steps * self.step_ns;
        }
        self.tick += steps;
        steps as u32
    }

    /// Номер последнего выполненного шага (сколько шагов прошло с начала).
    pub fn tick(&self) -> u64 {
        self.tick
    }

    /// Длительность шага.
    pub fn step(&self) -> Duration {
        Duration::from_nanos(self.step_ns)
    }

    /// Длительность шага в секундах (всегда одно и то же число для данной частоты).
    pub fn step_seconds(&self) -> f32 {
        (self.step_ns as f64 / 1e9) as f32
    }

    /// Доля шага, накопленная сверх последнего шага, в `[0, 1)`. Для
    /// интерполяции в рендере: `lerp(prev, curr, alpha)`. В симуляции не использовать.
    pub fn alpha(&self) -> f32 {
        (self.accumulator_ns as f64 / self.step_ns as f64) as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_match_real_time() {
        let mut t = FixedTimestep::battle();
        let mut total = 0;
        // 1000 кадров по 16.667 мс ≈ 16.667 с → 333 шага.
        for _ in 0..1000 {
            total += t.advance(Duration::from_nanos(16_666_667));
        }
        assert_eq!(total, 333);
        assert_eq!(t.tick(), 333);
        assert!((0.0..1.0).contains(&t.alpha()));
        assert_eq!(t.step_seconds(), 0.05);
    }

    #[test]
    fn spiral_of_death_is_capped() {
        let mut t = FixedTimestep::battle().with_max_steps(3);
        assert_eq!(t.advance(Duration::from_secs(10)), 3);
        assert!(t.alpha() < 1.0);
        assert_eq!(t.advance(Duration::from_millis(50)), 1);
    }
}
