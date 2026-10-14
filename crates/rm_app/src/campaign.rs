//! Режим «карта кампании» (`--campaign`): смотреть карту и щёлкать регионы.
//!
//! Сама карта (генерация, запекание, рисование) живёт в крейте
//! `rm_campaign_map`. Здесь — только то, что связывает её с окном:
//! мышь и клавиши, камера и надписи поверх карты.
//! Правил кампании (ходов, армий) тут пока нет — их делает `rm_campaign`.
//! Подробно: `docs/campaign-map/README.md`.

use std::collections::HashSet;
use std::path::Path;

use anyhow::{Context, Result};
use glam::{Vec2 as V2, Vec3};
use rm_campaign_map::gpu::{
    FLAG_FACTION_BORDERS, FLAG_GLOW, FLAG_REGION_LINES, MapConstants, ViewMode,
};
use rm_campaign_map::{
    BakedMap, CampaignMap, FactionStyle, MapCamera, MapConfig, Political, RegionId, SEA,
    TerrainMesh,
};
use rm_core::Rng;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::keyboard::{KeyCode, PhysicalKey};

use crate::game::Request;
use crate::hud::{self, Hud, WHITE};
use crate::render::UiRect;

/// Цвет за краем карты — глубокое море (тот же, что у самой глубокой воды при запекании).
const CLEAR: [f32; 4] = [0.0029, 0.0152, 0.0612, 1.0];
/// Сила окраски владельцев: выкл., как в оригинале, ярко.
const TINTS: [f32; 3] = [0.0, 0.18, 0.5];

pub struct CampaignView {
    map: CampaignMap,
    political: Political,
    camera: MapCamera,
    view: ViewMode,
    tint: usize,
    borders: bool,
    region_lines: bool,
    glow: bool,
    selected: RegionId,
    hover: RegionId,
    keys: HashSet<KeyCode>,
    cursor: V2,
    drag_last: Option<V2>,
    clock: f32,
    show_help: bool,
    demo: Option<Demo>,
    ui: Vec<UiRect>,
}

/// Демо для автоматических проверок: камера облетает карту, регионы
/// время от времени переходят к соседям — видно, что перекраска работает.
struct Demo {
    rng: Rng,
    next_change: f32,
}

impl CampaignView {
    /// Строит карту и возвращает её вместе с запечёнными текстурами и
    /// сеткой — их нужно один раз загрузить в видеопамять.
    pub fn new(data_dir: &Path, seed: u64, demo: bool) -> Result<(Self, BakedMap, TerrainMesh)> {
        let data = rm_data::GameData::load_dir(data_dir)
            .map_err(|e| anyhow::anyhow!("{e}"))
            .context("не удалось загрузить фракции из data/")?;
        let started = std::time::Instant::now();
        let map = CampaignMap::generate(&MapConfig {
            seed,
            ..MapConfig::default()
        });
        let baked = BakedMap::bake(&map);
        let mesh = TerrainMesh::build(&map);
        log::info!(
            "Карта кампании {0}×{0}: регионов {1}, треугольников {2}, текстуры {3:.1} МБ, построена за {4:.2} с",
            map.config.size,
            map.land_regions().len(),
            mesh.triangle_count(),
            [
                &baked.albedo,
                &baked.normal,
                &baked.light,
                &baked.border,
                &baked.region
            ]
            .iter()
            .map(|t| t.byte_len())
            .sum::<usize>() as f32
                / 1_048_576.0,
            started.elapsed().as_secs_f32()
        );
        let factions = data
            .factions
            .iter()
            .map(|f| FactionStyle {
                name: f.name.clone(),
                colour: [f.primary_colour.r, f.primary_colour.g, f.primary_colour.b],
            })
            .collect();
        let political = Political::generate(&map, factions, seed);
        let camera = MapCamera::new(map.config.world_size());
        let view = Self {
            map,
            political,
            camera,
            view: ViewMode::Final,
            tint: 1,
            borders: true,
            region_lines: true,
            glow: true,
            selected: SEA,
            hover: SEA,
            keys: HashSet::new(),
            cursor: V2::ZERO,
            drag_last: None,
            clock: 0.0,
            show_help: true,
            demo: demo.then(|| Demo {
                rng: Rng::new(seed),
                next_change: 0.5,
            }),
            ui: Vec::new(),
        };
        Ok((view, baked, mesh))
    }

    pub fn handle_event(&mut self, event: &WindowEvent, size: V2) -> Request {
        match event {
            WindowEvent::CursorMoved { position, .. } => {
                let p = V2::new(position.x as f32, position.y as f32);
                if let Some(last) = self.drag_last {
                    // Перетаскивание правой кнопкой: карта едет за мышью.
                    let d = (p - last) / size.y.max(1.0);
                    self.camera.pan(d.y * 0.8, -d.x * 0.8);
                    self.drag_last = Some(p);
                }
                self.cursor = p;
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let steps = match delta {
                    MouseScrollDelta::LineDelta(_, y) => *y,
                    MouseScrollDelta::PixelDelta(p) => p.y as f32 / 60.0,
                };
                // Приближаемся к точке под курсором, а не к центру экрана.
                let before = self.camera.distance;
                let aim = self.camera.pick(&self.map, self.cursor, size);
                self.camera.zoom(steps);
                if let Some(p) = aim {
                    let k = 1.0 - self.camera.distance / before;
                    let shift = (p - self.camera.target) * k;
                    self.camera.target.x += shift.x;
                    self.camera.target.z += shift.z;
                    self.camera.pan(0.0, 0.0);
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let pressed = *state == ElementState::Pressed;
                match (button, pressed) {
                    (MouseButton::Left, true) => {
                        self.selected = self.region_under(self.cursor, size);
                    }
                    (MouseButton::Right | MouseButton::Middle, true) => {
                        self.drag_last = Some(self.cursor)
                    }
                    (MouseButton::Right | MouseButton::Middle, false) => self.drag_last = None,
                    _ => {}
                }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                let PhysicalKey::Code(code) = event.physical_key else {
                    return Request::None;
                };
                if event.state == ElementState::Released {
                    self.keys.remove(&code);
                    return Request::None;
                }
                self.keys.insert(code);
                if !event.repeat {
                    return self.key_pressed(code);
                }
            }
            WindowEvent::Focused(false) => self.keys.clear(),
            _ => {}
        }
        Request::None
    }

    fn key_pressed(&mut self, code: KeyCode) -> Request {
        match code {
            KeyCode::Escape => {
                if self.selected != SEA {
                    self.selected = SEA;
                } else {
                    return Request::Quit;
                }
            }
            KeyCode::F1 => self.show_help = !self.show_help,
            KeyCode::KeyV | KeyCode::Tab => self.view = self.view.next(),
            KeyCode::KeyT => self.tint = (self.tint + 1) % TINTS.len(),
            KeyCode::KeyB => self.borders = !self.borders,
            KeyCode::KeyR => self.region_lines = !self.region_lines,
            KeyCode::KeyG => self.glow = !self.glow,
            KeyCode::KeyN => self.cycle_owner(),
            KeyCode::Home => self.camera = MapCamera::new(self.map.config.world_size()),
            _ => {}
        }
        Request::None
    }

    /// Передаёт выбранный регион следующей фракции — одна запись в таблицу (H-04).
    fn cycle_owner(&mut self) {
        if self.selected == SEA {
            return;
        }
        let n = self.political.factions.len() as u8;
        let next = (self.political.owner_of(self.selected) + 1) % n.max(1);
        self.political.set_owner(self.selected, next);
    }

    fn region_under(&self, cursor: V2, size: V2) -> RegionId {
        self.camera
            .pick(&self.map, cursor, size)
            .map_or(SEA, |p| self.map.region_at_world(p.x, p.z))
    }

    /// Камера с клавиатуры, наведение курсора, демо.
    pub fn update(&mut self, dt: f32, size: V2) {
        self.clock += dt;
        let held = |a: KeyCode, b: KeyCode| self.keys.contains(&a) || self.keys.contains(&b);
        let speed = dt * 0.9;
        let (mut fwd, mut right, mut turn) = (0.0, 0.0, 0.0);
        if held(KeyCode::KeyW, KeyCode::ArrowUp) {
            fwd += speed;
        }
        if held(KeyCode::KeyS, KeyCode::ArrowDown) {
            fwd -= speed;
        }
        if held(KeyCode::KeyD, KeyCode::ArrowRight) {
            right += speed;
        }
        if held(KeyCode::KeyA, KeyCode::ArrowLeft) {
            right -= speed;
        }
        if self.keys.contains(&KeyCode::KeyQ) {
            turn += dt * 1.2;
        }
        if self.keys.contains(&KeyCode::KeyE) {
            turn -= dt * 1.2;
        }
        if self.keys.contains(&KeyCode::PageUp) {
            self.camera.zoom(dt * 6.0);
        }
        if self.keys.contains(&KeyCode::PageDown) {
            self.camera.zoom(-dt * 6.0);
        }
        if let Some(demo) = self.demo.as_mut() {
            // Медленный облёт: камера кружит вокруг центра и приближается.
            let t = self.clock * 0.25;
            let w = self.map.config.world_size();
            self.camera.target.x = w * (0.5 + 0.25 * t.cos());
            self.camera.target.z = w * (0.5 + 0.2 * t.sin());
            self.camera.distance = w * (0.35 + 0.2 * (t * 0.7).cos());
            self.camera.yaw = 0.25 * (t * 0.5).sin();
            if self.clock >= demo.next_change {
                demo.next_change = self.clock + 0.6;
                let regions = self.map.land_regions();
                let r = &regions[demo.rng.below(regions.len() as u32) as usize];
                if let Some(&nb) = r.neighbours.first() {
                    let owner = self.political.owner_of(nb);
                    self.political.set_owner(r.id, owner);
                    self.selected = r.id;
                }
            }
        }
        self.camera.pan(fwd, right);
        self.camera.rotate(turn);
        self.camera.follow_ground(&self.map, dt);
        self.hover = if self.drag_last.is_none() {
            self.region_under(self.cursor, size)
        } else {
            SEA
        };
    }

    /// Данные кадра для шейдеров карты и список прямоугольников интерфейса.
    pub fn build_frame(&mut self, size: V2) -> (MapConstants, [f32; 4]) {
        let eye = self.camera.eye();
        let c = &self.map.config;
        let mut flags = 0;
        if self.borders {
            flags |= FLAG_FACTION_BORDERS;
        }
        if self.region_lines {
            flags |= FLAG_REGION_LINES;
        }
        if self.glow {
            flags |= FLAG_GLOW;
        }
        let sun = rm_campaign_map::bake::SUN;
        let constants = MapConstants {
            view_proj: self
                .camera
                .view_proj(size.x / size.y.max(1.0))
                .to_cols_array_2d(),
            eye: [eye.x, eye.y, eye.z, self.clock],
            sun: [sun[0], sun[1], sun[2], TINTS[self.tint]],
            map: [
                c.world_size(),
                c.size as f32,
                // Полуширина линии границы в пикселях: вдали чуть толще.
                1.0 + 0.6 * (self.camera.distance / 600.0).min(1.0),
                self.camera.distance * 1.4,
            ],
            ids: [
                u32::from(self.selected),
                u32::from(self.hover),
                self.view as u32,
                flags,
            ],
        };
        self.ui.clear();
        self.build_hud(size);
        (constants, CLEAR)
    }

    pub fn political(&self) -> &Political {
        &self.political
    }

    pub fn ui(&self) -> &[UiRect] {
        &self.ui
    }

    fn build_hud(&mut self, size: V2) {
        let scale = (size.y / 360.0).floor().clamp(2.0, 4.0);
        let small = (scale - 1.0).max(1.0);
        // Поселения: квадрат цвета владельца и название, пока камера не слишком далеко.
        let mut labels = Vec::new();
        for r in self.map.land_regions() {
            let (x, z) = self.map.world_of(r.settlement);
            let p = Vec3::new(x, self.map.display_height_at(x, z) + 0.6, z);
            let Some(px) = self.camera.project(p, size) else {
                continue;
            };
            if px.x < -50.0 || px.y < -50.0 || px.x > size.x + 50.0 || px.y > size.y + 50.0 {
                continue;
            }
            labels.push((r.id, px, r.name.to_uppercase()));
        }
        let names = self.camera.distance < 700.0;
        let mut h = Hud {
            rects: &mut self.ui,
        };
        for (id, px, name) in &labels {
            let colour = {
                let f = &self.political.factions[self.political.owner_of(*id) as usize];
                srgb(f.colour)
            };
            let s = small * 3.0;
            h.rect(
                px.x - s - 1.0,
                px.y - s - 1.0,
                s * 2.0 + 2.0,
                s * 2.0 + 2.0,
                [0.0, 0.0, 0.0, 0.85],
            );
            h.rect(px.x - s, px.y - s, s * 2.0, s * 2.0, colour);
            if *id == self.selected {
                h.frame(
                    px.x - s - 4.0,
                    px.y - s - 4.0,
                    s * 2.0 + 8.0,
                    s * 2.0 + 8.0,
                    2.0,
                    WHITE,
                );
            }
            if names || *id == self.selected || *id == self.hover {
                h.text_centered(px.x, px.y + s + 4.0, name, small, WHITE);
            }
        }

        // Заголовок и режим показа.
        let title = format!("КАРТА КАМПАНИИ — {}", self.view.title().to_uppercase());
        let tw = hud::text_width(&title, scale);
        h.rect(
            size.x * 0.5 - tw * 0.5 - 12.0,
            0.0,
            tw + 24.0,
            hud::text_height(scale) + 16.0,
            [0.05, 0.05, 0.08, 0.6],
        );
        h.text_centered(size.x * 0.5, 8.0, &title, scale, WHITE);
        if self.demo.is_some() {
            h.text_centered(
                size.x * 0.5,
                16.0 + hud::text_height(scale) + 6.0,
                "ДЕМО: РЕГИОНЫ ПЕРЕХОДЯТ К СОСЕДЯМ",
                scale,
                [1.0, 0.85, 0.3, 1.0],
            );
        }

        // Фракции справа: цвет, имя, число регионов.
        let lh = hud::text_height(small) + 8.0;
        let rows: Vec<(String, [f32; 4])> = self
            .political
            .factions
            .iter()
            .enumerate()
            .map(|(i, f)| {
                let title = if Some(i as u8) == self.political.rebels {
                    "МЯТЕЖНИКИ".to_string()
                } else {
                    f.name.replace('_', " ").to_uppercase()
                };
                (
                    format!("{title}: {}", self.political.count(i as u8)),
                    srgb(f.colour),
                )
            })
            .collect();
        let w = rows
            .iter()
            .map(|(t, _)| hud::text_width(t, small))
            .fold(0.0, f32::max)
            + 40.0;
        let x0 = size.x - w - 8.0;
        h.rect(
            x0,
            8.0,
            w,
            lh * rows.len() as f32 + 8.0,
            [0.05, 0.05, 0.08, 0.6],
        );
        for (i, (text, colour)) in rows.iter().enumerate() {
            let y = 12.0 + i as f32 * lh;
            h.rect(x0 + 8.0, y, 16.0, hud::text_height(small), *colour);
            h.text_shadow(x0 + 32.0, y, text, small, WHITE);
        }

        // Выбранный регион внизу слева.
        if self.selected != SEA {
            let r = &self.map.regions[self.selected as usize];
            let owner = self.political.owner_of(r.id);
            let f = &self.political.factions[owner as usize];
            let lines = [
                r.name.to_uppercase(),
                format!("ВЛАДЕЛЕЦ: {}", f.name.replace('_', " ").to_uppercase()),
                format!("СОСЕДЕЙ: {}   ПЛОЩАДЬ: {}", r.neighbours.len(), r.area),
                "N — ПЕРЕДАТЬ СЛЕДУЮЩЕЙ ФРАКЦИИ".to_string(),
            ];
            let w = lines
                .iter()
                .map(|l| hud::text_width(l, small))
                .fold(0.0, f32::max)
                + 24.0;
            let lh = hud::text_height(small) + 8.0;
            let y0 = size.y - lh * lines.len() as f32 - 20.0;
            h.rect(
                8.0,
                y0,
                w,
                lh * lines.len() as f32 + 12.0,
                [0.05, 0.05, 0.08, 0.7],
            );
            h.rect(8.0, y0, 6.0, lh * lines.len() as f32 + 12.0, srgb(f.colour));
            for (i, l) in lines.iter().enumerate() {
                h.text_shadow(20.0, y0 + 8.0 + i as f32 * lh, l, small, WHITE);
            }
        }

        if self.show_help {
            let lines = [
                "WASD/стрелки — камера, Q/E — поворот",
                "колесо, PgUp/PgDn — приближение",
                "ПКМ/СКМ + мышь — тащить карту",
                "ЛКМ — выбрать регион, N — сменить владельца",
                "V — слой: карта, albedo, тени, нормали...",
                "T — окраска владельцев, B — границы",
                "R — линии регионов, G — полосы у границ",
                "Home — вся карта, F1 — подсказка, Esc — выход",
            ];
            let lh = hud::text_height(small) + 6.0;
            let w = lines
                .iter()
                .map(|l| hud::text_width(l, small))
                .fold(0.0, f32::max)
                + 16.0;
            h.rect(
                8.0,
                8.0,
                w,
                lh * lines.len() as f32 + 10.0,
                [0.05, 0.05, 0.08, 0.6],
            );
            for (i, l) in lines.iter().enumerate() {
                h.text_shadow(16.0, 14.0 + i as f32 * lh, l, small, WHITE);
            }
        }
    }
}

/// Цвет из файла данных (sRGB, 0..255) в линейный для шейдеров.
fn srgb(c: [u8; 3]) -> [f32; 4] {
    let lin = |c: u8| (f32::from(c) / 255.0).powf(2.2);
    [lin(c[0]), lin(c[1]), lin(c[2]), 1.0]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data_dir() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data")
    }

    #[test]
    fn campaign_view_builds_from_data() {
        let (mut view, baked, mesh) = CampaignView::new(&data_dir(), 1, true).unwrap();
        assert!(mesh.triangle_count() > 0);
        assert_eq!(baked.region.size, view.map.config.size);
        // Фракции взяты из descr_sm_factions.txt, мятежники узнаны.
        assert!(
            view.political
                .factions
                .iter()
                .any(|f| f.name == "northmark")
        );
        assert!(view.political.rebels.is_some());
        let size = V2::new(1280.0, 720.0);
        for _ in 0..30 {
            view.update(0.1, size);
        }
        let (c, _) = view.build_frame(size);
        assert_eq!(c.ids[2], ViewMode::Final as u32);
        assert!(!view.ui().is_empty());
        // Демо успело передать хотя бы один регион.
        assert!(view.political.revision > 0);
    }
}
