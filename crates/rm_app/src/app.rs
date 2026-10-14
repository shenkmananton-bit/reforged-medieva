//! Приложение: окно, события и главный цикл.
//!
//! winit сообщает о событиях (окно создано, изменён размер, нажата клавиша,
//! пора рисовать). События мыши и клавиатуры уходят в [`Game`]. На каждой
//! итерации цикла мы:
//! 1) считаем, сколько реального времени прошло (с учётом паузы и скорости игры);
//! 2) выполняем нужное число шагов битвы с фиксированным шагом;
//! 3) рисуем кадр, смешивая два последних состояния битвы.
//!
//! Подробно: `docs/app/main-loop.md`.

use std::time::Instant;

use anyhow::Result;
use glam::Vec2 as V2;
use winit::application::ApplicationHandler;
use winit::dpi::PhysicalSize;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow};
use winit::window::{Window, WindowId};

use crate::game::{Game, Request};
use crate::options::Options;
use crate::render::{Frame, FrameOutcome, Renderer};
use crate::timing::{FixedTimestep, RateCounter};

const TITLE: &str = "Reforged Medieval";

/// Максимум тиков симуляции за один кадр (защита от «спирали смерти»).
const MAX_TICKS_PER_FRAME: u32 = 5;

pub struct App {
    options: Options,
    // Рендер объявлен раньше окна: поля удаляются по порядку объявления,
    // а поверхность Vulkan должна исчезнуть раньше окна.
    renderer: Option<Renderer>,
    window: Option<Window>,
    timestep: FixedTimestep,
    rates: RateCounter,
    last_frame: Option<Instant>,
    ticks_this_frame: u32,
    game: Game,
    frames_presented: u64,
    /// Ошибка, из-за которой пришлось остановиться; main выведет её.
    pub error: Option<anyhow::Error>,
}

impl App {
    pub fn new(options: Options, game: Game) -> Self {
        Self {
            timestep: FixedTimestep::new(options.tick_rate, MAX_TICKS_PER_FRAME),
            options,
            renderer: None,
            window: None,
            rates: RateCounter::default(),
            last_frame: None,
            ticks_this_frame: 0,
            game,
            frames_presented: 0,
            error: None,
        }
    }

    fn init(&mut self, event_loop: &ActiveEventLoop) -> Result<()> {
        let attrs = Window::default_attributes()
            .with_title(TITLE)
            .with_inner_size(PhysicalSize::new(self.options.width, self.options.height))
            .with_min_inner_size(PhysicalSize::new(320, 200));
        let window = event_loop.create_window(attrs)?;
        let renderer = Renderer::new(&window, self.options.vsync)?;
        log::info!(
            "Окно {}×{}, симуляция {} тиков/с, vsync: {}",
            self.options.width,
            self.options.height,
            self.options.tick_rate,
            self.options.vsync
        );
        self.renderer = Some(renderer);
        self.window = Some(window);
        Ok(())
    }

    /// Шаги 1–2 цикла: время и тики симуляции.
    fn update(&mut self) {
        let now = Instant::now();
        let elapsed = self.last_frame.map_or_else(Default::default, |t| now - t);
        self.last_frame = Some(now);

        // Пауза и ускорение: игровое время = реальное × множитель.
        let game_elapsed = elapsed.mul_f32(self.game.time_scale());
        let ticks = self.timestep.advance(game_elapsed);
        for _ in 0..ticks {
            self.game.tick();
        }
        self.game.update(elapsed.as_secs_f32());
        self.ticks_this_frame += ticks;

        if let Some(rates) = self
            .rates
            .record(elapsed, std::mem::take(&mut self.ticks_this_frame))
            && let (Some(window), Some(renderer)) = (&self.window, &self.renderer)
        {
            window.set_title(&format!(
                "{TITLE} — {:.0} FPS, {:.0} тиков/с — {}",
                rates.fps,
                rates.tps,
                renderer.gpu_name()
            ));
        }
    }

    /// Шаг 3 цикла: кадр.
    fn render(&mut self, event_loop: &ActiveEventLoop) -> Result<()> {
        let Some(renderer) = self.renderer.as_mut() else {
            return Ok(());
        };
        let (w, h) = renderer.extent();
        let size = V2::new(w as f32, h as f32);
        let (constants, clear) = self.game.build_frame(self.timestep.alpha(), size);
        let (meshes, decals, ui) = self.game.frame_lists();
        let frame = Frame {
            clear,
            constants,
            meshes,
            decals,
            ui,
        };
        if renderer.draw_frame(&frame)? == FrameOutcome::Presented {
            self.frames_presented += 1;
        }
        if let Some(limit) = self.options.exit_after_frames
            && self.frames_presented >= limit
        {
            log::info!(
                "Нарисовано {} кадров, шагов битвы: {}. Выход.",
                self.frames_presented,
                self.game.battle().tick()
            );
            event_loop.exit();
        }
        Ok(())
    }

    fn fail(&mut self, event_loop: &ActiveEventLoop, err: anyhow::Error) {
        self.error = Some(err);
        event_loop.exit();
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        // Игра постоянно обновляется, поэтому не ждём событий, а крутим цикл.
        event_loop.set_control_flow(ControlFlow::Poll);
        if self.window.is_none()
            && let Err(e) = self.init(event_loop)
        {
            self.fail(event_loop, e);
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if let Some(r) = self.renderer.as_mut() {
                    r.resize(size.width, size.height);
                }
            }
            WindowEvent::RedrawRequested => {
                self.update();
                if let Err(e) = self.render(event_loop) {
                    self.fail(event_loop, e);
                }
            }
            other => {
                let size = self.renderer.as_ref().map_or(V2::ONE, |r| {
                    let (w, h) = r.extent();
                    V2::new(w as f32, h as f32)
                });
                if self.game.handle_event(&other, size) == Request::Quit {
                    event_loop.exit();
                }
            }
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        // Все события обработаны — просим следующий кадр.
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }

    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        // Явно освобождаем GPU-ресурсы до закрытия окна.
        self.renderer = None;
    }
}
