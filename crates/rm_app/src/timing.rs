//! Время в главном цикле: фиксированный шаг симуляции и счётчик кадров.
//!
//! Симуляция (логика игры) идёт с постоянной частотой, например 20 тиков
//! в секунду, независимо от того, сколько кадров в секунду рисует видеокарта.
//! Так игра ведёт себя одинаково на быстром и медленном компьютере.
//! Подробное объяснение: `docs/app/main-loop.md`.

use std::time::Duration;

/// Накопитель времени для фиксированного шага симуляции
/// (классическая схема «Fix Your Timestep»).
#[derive(Debug, Clone)]
pub struct FixedTimestep {
    /// Длительность одного тика симуляции.
    tick: Duration,
    /// Сколько реального времени накопилось и ещё не «прожито» симуляцией.
    accumulator: Duration,
    /// Максимум тиков за один кадр. Защита от «спирали смерти»: если кадр
    /// длился очень долго (окно перетаскивали, отладчик), не пытаемся
    /// догнать всё сразу, а отбрасываем лишнее время.
    max_ticks_per_frame: u32,
}

impl FixedTimestep {
    /// Создаёт шаг с частотой `ticks_per_second` (должна быть больше нуля).
    pub fn new(ticks_per_second: u32, max_ticks_per_frame: u32) -> Self {
        assert!(ticks_per_second > 0, "частота симуляции должна быть > 0");
        Self {
            tick: Duration::from_secs_f64(1.0 / f64::from(ticks_per_second)),
            accumulator: Duration::ZERO,
            max_ticks_per_frame: max_ticks_per_frame.max(1),
        }
    }

    /// Добавляет прошедшее реальное время и возвращает, сколько тиков
    /// симуляции нужно выполнить прямо сейчас.
    pub fn advance(&mut self, elapsed: Duration) -> u32 {
        self.accumulator += elapsed;
        let mut ticks = 0;
        while self.accumulator >= self.tick && ticks < self.max_ticks_per_frame {
            self.accumulator -= self.tick;
            ticks += 1;
        }
        if ticks == self.max_ticks_per_frame && self.accumulator >= self.tick {
            // Не успеваем: выбрасываем остаток, чтобы не копить отставание.
            self.accumulator = Duration::ZERO;
        }
        ticks
    }

    /// Доля следующего тика, уже прошедшая к моменту рисования (от 0 до 1).
    /// Рендер использует её, чтобы плавно смешать предыдущее и текущее
    /// состояние мира (интерполяция), иначе движение дёргалось бы с частотой тиков.
    pub fn alpha(&self) -> f32 {
        (self.accumulator.as_secs_f64() / self.tick.as_secs_f64()) as f32
    }
}

/// Считает кадры и тики за последнюю секунду, чтобы показать их в заголовке окна.
#[derive(Debug, Default)]
pub struct RateCounter {
    window: Duration,
    frames: u32,
    ticks: u32,
}

/// Результат замера за прошедшую секунду.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rates {
    pub fps: f64,
    pub tps: f64,
}

impl RateCounter {
    /// Учитывает один нарисованный кадр, `ticks` выполненных тиков и время кадра.
    /// Раз в секунду возвращает посчитанные частоты.
    pub fn record(&mut self, elapsed: Duration, ticks: u32) -> Option<Rates> {
        self.window += elapsed;
        self.frames += 1;
        self.ticks += ticks;
        if self.window < Duration::from_secs(1) {
            return None;
        }
        let secs = self.window.as_secs_f64();
        let rates = Rates {
            fps: f64::from(self.frames) / secs,
            tps: f64::from(self.ticks) / secs,
        };
        *self = Self::default();
        Some(rates)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(v: u64) -> Duration {
        Duration::from_millis(v)
    }

    #[test]
    fn short_frames_accumulate_until_a_tick() {
        let mut ts = FixedTimestep::new(20, 5); // тик = 50 мс
        assert_eq!(ts.advance(ms(16)), 0);
        assert_eq!(ts.advance(ms(16)), 0);
        assert_eq!(ts.advance(ms(16)), 0);
        assert_eq!(ts.advance(ms(16)), 1); // 64 мс >= 50 мс
        assert!((ts.alpha() - 14.0 / 50.0).abs() < 1e-3);
    }

    #[test]
    fn long_frame_runs_several_ticks() {
        let mut ts = FixedTimestep::new(20, 5);
        assert_eq!(ts.advance(ms(120)), 2);
        assert!((ts.alpha() - 0.4).abs() < 1e-3);
    }

    #[test]
    fn huge_frame_is_capped_and_backlog_dropped() {
        let mut ts = FixedTimestep::new(20, 5);
        assert_eq!(ts.advance(Duration::from_secs(10)), 5);
        assert_eq!(ts.alpha(), 0.0);
        assert_eq!(ts.advance(ms(10)), 0);
    }

    #[test]
    fn tick_rate_is_independent_of_frame_rate() {
        // 1 секунда кадрами по 7 мс и кадрами по 33 мс даёт одинаковые 20 тиков.
        for frame in [7u64, 33] {
            let mut ts = FixedTimestep::new(20, 5);
            let mut total = 0;
            let mut t = 0;
            while t + frame <= 1000 {
                total += ts.advance(ms(frame));
                t += frame;
            }
            total += ts.advance(ms(1000 - t));
            assert_eq!(total, 20, "кадр {frame} мс");
        }
    }

    #[test]
    fn rate_counter_reports_once_per_second() {
        let mut rc = RateCounter::default();
        for _ in 0..59 {
            assert_eq!(rc.record(ms(16), 0), None);
        }
        let r = rc.record(ms(56), 20).expect("секунда прошла");
        assert!((r.fps - 60.0).abs() < 1e-9);
        assert!((r.tps - 20.0).abs() < 1e-9);
    }
}
