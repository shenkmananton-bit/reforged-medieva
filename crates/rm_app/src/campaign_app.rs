//! Окно и главный цикл режима карты кампании.
//!
//! Устроено как [`crate::app::App`] у битвы, но проще: симуляции пока нет,
//! каждый кадр — это «обработать ввод, сдвинуть камеру, нарисовать карту».
//! Отдельный файл, чтобы не трогать главный цикл битвы.

use std::time::Instant;

use anyhow::Result;
use glam::Vec2 as V2;
use rm_campaign_map::{BakedMap, TerrainMesh};
use winit::application::ApplicationHandler;
use winit::dpi::PhysicalSize;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow};
use winit::window::{Window, WindowId};

use crate::campaign::CampaignView;
use crate::game::Request;
use crate::options::Options;
use crate::render::mesh::{Instance, MESH_COUNT};
use crate::render::{CampaignFrame, Frame, FrameConstants, FrameOutcome, Renderer};
use crate::timing::RateCounter;

const TITLE: &str = "Reforged Medieval — карта кампании";

pub struct CampaignApp {
    options: Options,
    // Рендер раньше окна: поверхность Vulkan должна исчезнуть раньше окна.
    renderer: Option<Renderer>,
    window: Option<Window>,
    view: CampaignView,
    /// Текстуры и сетка ждут, пока появится видеокарта; после загрузки не нужны.
    upload: Option<(BakedMap, TerrainMesh)>,
    rates: RateCounter,
    last_frame: Option<Instant>,
    frames_presented: u64,
    no_meshes: [Vec<Instance>; MESH_COUNT],
    pub error: Option<anyhow::Error>,
}

impl CampaignApp {
    pub fn new(options: Options, view: CampaignView, baked: BakedMap, mesh: TerrainMesh) -> Self {
        Self {
            options,
            renderer: None,
            window: None,
            view,
            upload: Some((baked, mesh)),
            rates: RateCounter::default(),
            last_frame: None,
            frames_presented: 0,
            no_meshes: Default::default(),
            error: None,
        }
    }

    fn init(&mut self, event_loop: &ActiveEventLoop) -> Result<()> {
        let attrs = Window::default_attributes()
            .with_title(TITLE)
            .with_inner_size(PhysicalSize::new(self.options.width, self.options.height))
            .with_min_inner_size(PhysicalSize::new(320, 200));
        let window = event_loop.create_window(attrs)?;
        let mut renderer = Renderer::new(&window, self.options.vsync)?;
        if let Some((baked, mesh)) = self.upload.take() {
            renderer.load_campaign_map(&baked, &mesh)?;
        }
        self.renderer = Some(renderer);
        self.window = Some(window);
        Ok(())
    }

    fn size(&self) -> V2 {
        self.renderer.as_ref().map_or(V2::ONE, |r| {
            let (w, h) = r.extent();
            V2::new(w as f32, h as f32)
        })
    }

    fn frame(&mut self, event_loop: &ActiveEventLoop) -> Result<()> {
        let now = Instant::now();
        let elapsed = self.last_frame.map_or_else(Default::default, |t| now - t);
        self.last_frame = Some(now);
        let size = self.size();
        self.view.update(elapsed.as_secs_f32().min(0.1), size);

        if let Some(rates) = self.rates.record(elapsed, 0)
            && let (Some(window), Some(renderer)) = (&self.window, &self.renderer)
        {
            window.set_title(&format!(
                "{TITLE} — {:.0} FPS — {}",
                rates.fps,
                renderer.gpu_name()
            ));
        }

        let Some(renderer) = self.renderer.as_mut() else {
            return Ok(());
        };
        let (constants, clear) = self.view.build_frame(size);
        let frame = Frame {
            clear,
            // Земля и модели битвы в этом режиме не рисуются.
            constants: FrameConstants {
                view_proj: constants.view_proj,
                sun: [0.0; 4],
                eye: [0.0; 4],
                fog: [0.0, 0.0, 0.0, 1.0],
            },
            meshes: &self.no_meshes,
            decals: &[],
            ui: self.view.ui(),
            campaign: Some(CampaignFrame {
                constants,
                political: self.view.political(),
            }),
        };
        if renderer.draw_frame(&frame)? == FrameOutcome::Presented {
            self.frames_presented += 1;
        }
        if let Some(limit) = self.options.exit_after_frames
            && self.frames_presented >= limit
        {
            log::info!("Нарисовано {} кадров карты. Выход.", self.frames_presented);
            event_loop.exit();
        }
        Ok(())
    }

    fn fail(&mut self, event_loop: &ActiveEventLoop, err: anyhow::Error) {
        self.error = Some(err);
        event_loop.exit();
    }
}

impl ApplicationHandler for CampaignApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
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
                if let Err(e) = self.frame(event_loop) {
                    self.fail(event_loop, e);
                }
            }
            other => {
                let size = self.size();
                if self.view.handle_event(&other, size) == Request::Quit {
                    event_loop.exit();
                }
            }
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }

    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        self.renderer = None;
    }
}
