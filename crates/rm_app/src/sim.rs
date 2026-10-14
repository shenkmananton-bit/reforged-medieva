//! Временная «игровая» симуляция для каркаса приложения.
//!
//! Настоящая логика игры появится в `rm_core` (ECS). Здесь только
//! демонстрация связки «тик симуляции → интерполяция → кадр»:
//! симуляция медленно двигает фазу, а рендер превращает её в цвет фона.

/// Состояние демо-симуляции.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct DemoSim {
    /// Номер выполненного тика.
    pub tick: u64,
    /// Фаза «дыхания» цвета фона, от 0 до 1.
    pub phase: f32,
}

/// Сколько секунд длится один цикл смены цвета.
const CYCLE_SECONDS: f32 = 8.0;

impl DemoSim {
    /// Один шаг симуляции длительностью `dt_seconds`.
    pub fn step(&mut self, dt_seconds: f32) {
        self.tick += 1;
        self.phase = (self.phase + dt_seconds / CYCLE_SECONDS).fract();
    }
}

/// Плавно смешивает два состояния симуляции для рисования.
/// `alpha` = 0 даёт `prev`, `alpha` = 1 даёт `curr`.
pub fn interpolated_phase(prev: &DemoSim, curr: &DemoSim, alpha: f32) -> f32 {
    let mut a = prev.phase;
    let b = curr.phase;
    // Переход через 1.0 → 0.0: «разворачиваем» фазу, чтобы не мигнуть.
    if b < a {
        a -= 1.0;
    }
    (a + (b - a) * alpha.clamp(0.0, 1.0)).rem_euclid(1.0)
}

/// Цвет фона по фазе: тёмно-синие сумерки, медленно теплеющие до золота.
pub fn clear_color(phase: f32) -> [f32; 4] {
    let dusk = [0.04, 0.05, 0.12];
    let gold = [0.22, 0.14, 0.05];
    // Плавная волна 0 → 1 → 0 за один цикл.
    let t = 0.5 - 0.5 * (phase * std::f32::consts::TAU).cos();
    [
        dusk[0] + (gold[0] - dusk[0]) * t,
        dusk[1] + (gold[1] - dusk[1]) * t,
        dusk[2] + (gold[2] - dusk[2]) * t,
        1.0,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn step_advances_tick_and_wraps_phase() {
        let mut s = DemoSim::default();
        for _ in 0..(8 * 20) {
            s.step(1.0 / 20.0);
        }
        assert_eq!(s.tick, 160);
        assert!(!(1e-3..=1.0 - 1e-3).contains(&s.phase));
    }

    #[test]
    fn interpolation_handles_wraparound() {
        let prev = DemoSim {
            tick: 0,
            phase: 0.95,
        };
        let curr = DemoSim {
            tick: 1,
            phase: 0.05,
        };
        let mid = interpolated_phase(&prev, &curr, 0.5);
        assert!(!(1e-3..=1.0 - 1e-3).contains(&mid), "получили {mid}");
    }

    #[test]
    fn color_stays_in_range() {
        for i in 0..100 {
            let c = clear_color(i as f32 / 100.0);
            assert!(c.iter().all(|v| (0.0..=1.0).contains(v)));
        }
    }
}
