//! Игра: битва, управление мышью и клавиатурой, сборка кадра.
//!
//! Здесь склеиваются симуляция (`rm_battle`), ИИ (`rm_battle_ai`) и рендер.
//! Ввод игрока не меняет битву напрямую: он превращается в команды
//! [`Command`], которые применяются на ближайшем шаге симуляции, так же
//! как решения ИИ (`docs/adr/determinism.md`).
//! Как играть: `docs/how-to-run.md`. Как устроено: `docs/app/battle-ui.md`.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use glam::{Vec2 as V2, Vec3};
use rm_battle::{
    ArmySetup, Battle, BattleConfig, BattleSetup, Command, Order, Outcome, Regiment, Side, UnitId,
    UnitSetup, UnitState, Vec2,
};
use rm_battle_ai::BattleAi;
use rm_data::GameData;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::keyboard::{KeyCode, PhysicalKey};

use crate::camera::{Camera, to_field, to_world};
use crate::hud::{self, Hud, WHITE};
use crate::render::mesh::{Instance, MESH_COUNT, MeshId};
use crate::render::{FrameConstants, UiRect};

/// Сторона игрока.
const PLAYER: Side = 0;
/// Цвет неба и тумана.
const SKY: [f32; 4] = [0.50, 0.62, 0.78, 1.0];
/// Двойной щелчок: не дольше этого, секунды.
const DOUBLE_CLICK: f32 = 0.35;

/// Что игра просит у приложения после события.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Request {
    None,
    Quit,
}

/// Состояние кнопок мыши и клавиш.
#[derive(Default)]
struct Input {
    cursor: V2,
    left_start: Option<V2>,
    right_start: Option<(V2, Option<Vec2>)>,
    middle_last: Option<V2>,
    keys: HashSet<KeyCode>,
    shift: bool,
    ctrl: bool,
    last_right: Option<(f32, V2)>,
}

/// Временная метка на земле после приказа.
struct Marker {
    pos: Vec2,
    born: f32,
    color: [f32; 4],
}

pub struct Game {
    data: GameData,
    seed: u64,
    demo: bool,
    battle: Battle,
    enemy_ai: BattleAi,
    player_ai: Option<BattleAi>,
    pending: Vec<Command>,
    selected: Vec<bool>,
    /// Цвета фракций в линейном пространстве: [сторона][основной, второй].
    colors: [[[f32; 4]; 2]; 2],
    pub camera: Camera,
    input: Input,
    paused: bool,
    speed: f32,
    show_help: bool,
    /// Реальное время с начала битвы, секунды (для анимаций интерфейса).
    clock: f32,
    markers: Vec<Marker>,
    meshes: [Vec<Instance>; MESH_COUNT],
    decals: Vec<Instance>,
    ui: Vec<UiRect>,
}

impl Game {
    /// `demo` — обе армии под управлением ИИ (для показа и проверок на CI).
    pub fn new(data_dir: &Path, seed: u64, demo: bool) -> Result<Self> {
        let data = GameData::load_dir(data_dir)
            .map_err(|e| anyhow::anyhow!("ошибки в данных {}:\n{e}", data_dir.display()))?;
        let battle = new_battle(&data, seed)?;
        let colors = [0, 1].map(|side| {
            let f = data
                .faction(battle.faction(side))
                .expect("фракции проверены при создании битвы");
            [
                srgb(f.primary_colour.r, f.primary_colour.g, f.primary_colour.b),
                srgb(
                    f.secondary_colour.r,
                    f.secondary_colour.g,
                    f.secondary_colour.b,
                ),
            ]
        });
        let units = battle.units().len();
        let mut game = Self {
            data,
            seed,
            demo,
            battle,
            enemy_ai: BattleAi::new(1),
            player_ai: demo.then(|| BattleAi::new(PLAYER)),
            pending: Vec::new(),
            selected: vec![false; units],
            colors,
            camera: Camera::new(Vec3::new(0.0, 0.0, 80.0), 0.0),
            input: Input::default(),
            paused: false,
            speed: 1.0,
            show_help: true,
            clock: 0.0,
            markers: Vec::new(),
            meshes: Default::default(),
            decals: Vec::new(),
            ui: Vec::new(),
        };
        game.reset_camera();
        Ok(game)
    }

    fn restart(&mut self) -> Result<()> {
        self.seed = self.seed.wrapping_add(1);
        self.battle = new_battle(&self.data, self.seed)?;
        self.enemy_ai = BattleAi::new(1);
        self.player_ai = self.demo.then(|| BattleAi::new(PLAYER));
        self.pending.clear();
        self.selected = vec![false; self.battle.units().len()];
        self.markers.clear();
        self.paused = false;
        self.reset_camera();
        Ok(())
    }

    fn reset_camera(&mut self) {
        // За спиной своей армии, с видом на поле.
        let mine = side_center(&self.battle, PLAYER);
        self.camera = Camera::new(to_world(mine + Vec2::NORTH * 30.0, 0.0), 0.0);
        self.camera.zoom(1.5);
    }

    /// Во сколько раз игровое время быстрее реального (0 — пауза).
    pub fn time_scale(&self) -> f32 {
        if self.paused { 0.0 } else { self.speed }
    }

    /// Один шаг симуляции: команды игрока и ИИ, затем шаг битвы.
    pub fn tick(&mut self) {
        let mut cmds = std::mem::take(&mut self.pending);
        cmds.extend(self.enemy_ai.think(&self.battle));
        if let Some(ai) = &mut self.player_ai {
            cmds.extend(ai.think(&self.battle));
        }
        self.battle.step(&cmds);
        // Погибшие и бежавшие отряды из выделения убираем.
        for (sel, u) in self.selected.iter_mut().zip(self.battle.units()) {
            *sel &= u.is_fighting();
        }
    }

    pub fn battle(&self) -> &Battle {
        &self.battle
    }

    /// Камера, клавиши перемещения; `dt` — реальное время кадра.
    pub fn update(&mut self, dt: f32) {
        self.clock += dt;
        let keys = &self.input.keys;
        let held = |a: KeyCode, b: KeyCode| keys.contains(&a) || keys.contains(&b);
        let (fwd, right) = self.camera.ground_axes();
        let mut pan = Vec3::ZERO;
        if held(KeyCode::KeyW, KeyCode::ArrowUp) {
            pan += fwd;
        }
        if held(KeyCode::KeyS, KeyCode::ArrowDown) {
            pan -= fwd;
        }
        if held(KeyCode::KeyD, KeyCode::ArrowRight) {
            pan += right;
        }
        if held(KeyCode::KeyA, KeyCode::ArrowLeft) {
            pan -= right;
        }
        let pan_speed = 25.0 + self.camera.distance * 0.9;
        let boost = if self.input.shift { 2.5 } else { 1.0 };
        self.camera.target += pan.normalize_or_zero() * pan_speed * boost * dt;
        let lim = self.battle.config().field_half_size + 80.0;
        self.camera.target.x = self.camera.target.x.clamp(-lim, lim);
        self.camera.target.z = self.camera.target.z.clamp(-lim, lim);
        if keys.contains(&KeyCode::KeyQ) {
            self.camera.yaw += 1.6 * dt;
        }
        if keys.contains(&KeyCode::KeyE) {
            self.camera.yaw -= 1.6 * dt;
        }
        self.markers.retain(|m| self.clock - m.born < 0.8);
    }

    pub fn handle_event(&mut self, event: &WindowEvent, size: V2) -> Request {
        match event {
            WindowEvent::ModifiersChanged(m) => {
                self.input.shift = m.state().shift_key();
                self.input.ctrl = m.state().control_key();
            }
            WindowEvent::CursorMoved { position, .. } => {
                let p = V2::new(position.x as f32, position.y as f32);
                if let Some(last) = self.input.middle_last {
                    let d = p - last;
                    self.camera.yaw -= d.x * 0.006;
                    self.camera.pitch = (self.camera.pitch + d.y * 0.004).clamp(0.15, 1.45);
                    self.input.middle_last = Some(p);
                }
                self.input.cursor = p;
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let steps = match delta {
                    MouseScrollDelta::LineDelta(_, y) => *y,
                    MouseScrollDelta::PixelDelta(p) => p.y as f32 / 60.0,
                };
                // Приближаемся к точке под курсором, а не к центру экрана.
                let before = self.camera.distance;
                self.camera.zoom(steps);
                if let Some(p) = self.camera.pick_plane(self.input.cursor, size, 0.0) {
                    let k = 1.0 - self.camera.distance / before;
                    self.camera.target += (p - self.camera.target) * k;
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let pressed = *state == ElementState::Pressed;
                let cursor = self.input.cursor;
                match (button, pressed) {
                    (MouseButton::Left, true) => self.input.left_start = Some(cursor),
                    (MouseButton::Left, false) => {
                        if let Some(start) = self.input.left_start.take() {
                            self.left_click(start, cursor, size);
                        }
                    }
                    (MouseButton::Right, true) => {
                        let ground = self.ground_at(cursor, size);
                        self.input.right_start = Some((cursor, ground));
                    }
                    (MouseButton::Right, false) => {
                        if let Some((start, ground)) = self.input.right_start.take() {
                            self.right_click(start, ground, cursor, size);
                        }
                    }
                    (MouseButton::Middle, true) => self.input.middle_last = Some(cursor),
                    (MouseButton::Middle, false) => self.input.middle_last = None,
                    _ => {}
                }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                let PhysicalKey::Code(code) = event.physical_key else {
                    return Request::None;
                };
                if event.state == ElementState::Released {
                    self.input.keys.remove(&code);
                    return Request::None;
                }
                self.input.keys.insert(code);
                if event.repeat {
                    return Request::None;
                }
                return self.key_pressed(code);
            }
            WindowEvent::Focused(false) => {
                self.input.keys.clear();
            }
            _ => {}
        }
        Request::None
    }

    fn key_pressed(&mut self, code: KeyCode) -> Request {
        match code {
            KeyCode::Escape => {
                if self.selected.iter().any(|&s| s) {
                    self.selected.fill(false);
                } else {
                    return Request::Quit;
                }
            }
            KeyCode::Space | KeyCode::KeyP => self.paused = !self.paused,
            KeyCode::Equal | KeyCode::NumpadAdd => self.speed = (self.speed * 2.0).min(8.0),
            KeyCode::Minus | KeyCode::NumpadSubtract => self.speed = (self.speed * 0.5).max(0.25),
            KeyCode::F1 => self.show_help = !self.show_help,
            KeyCode::Home => self.reset_camera(),
            KeyCode::KeyA if self.input.ctrl => {
                for (sel, u) in self.selected.iter_mut().zip(self.battle.units()) {
                    *sel = u.side == PLAYER && u.is_fighting();
                }
            }
            KeyCode::Backspace => {
                for id in self.selected_ids() {
                    self.pending.push(Command::Halt { unit: id });
                }
            }
            KeyCode::Enter | KeyCode::F5 => {
                if (code == KeyCode::F5 || self.battle.outcome().is_some())
                    && let Err(e) = self.restart()
                {
                    log::error!("не удалось начать битву заново: {e:#}");
                }
            }
            _ => {
                if let Some(n) = digit(code) {
                    let mine: Vec<UnitId> = self.player_units().map(|u| u.id).collect();
                    if let Some(&id) = mine.get(n) {
                        if !self.input.shift {
                            self.selected.fill(false);
                        }
                        self.selected[id.index()] = self.battle.units()[id.index()].is_fighting();
                    }
                }
            }
        }
        Request::None
    }

    fn player_units(&self) -> impl Iterator<Item = &Regiment> {
        self.battle.units().iter().filter(|u| u.side == PLAYER)
    }

    fn selected_ids(&self) -> Vec<UnitId> {
        self.battle
            .units()
            .iter()
            .filter(|u| self.selected[u.id.index()])
            .map(|u| u.id)
            .collect()
    }

    fn ground_at(&self, cursor: V2, size: V2) -> Option<Vec2> {
        self.camera.pick_plane(cursor, size, 0.0).map(to_field)
    }

    /// Отряд под курсором: ближайший солдат к точке луча на высоте груди или земли.
    fn unit_at(&self, cursor: V2, size: V2) -> Option<UnitId> {
        let mut best: Option<(UnitId, f32)> = None;
        for height in [1.0, 0.0] {
            let Some(p) = self.camera.pick_plane(cursor, size, height).map(to_field) else {
                continue;
            };
            for (s, b) in self.battle.soldiers() {
                let r = &self.battle.units()[s.unit.index()];
                let reach = r.stats.radius + 0.9;
                let d = b.pos.distance(p);
                if d < reach && best.is_none_or(|(_, bd)| d < bd) {
                    best = Some((s.unit, d));
                }
            }
            if best.is_some() {
                break;
            }
        }
        best.map(|(id, _)| id)
    }

    fn left_click(&mut self, start: V2, end: V2, size: V2) {
        let add = self.input.shift || self.input.ctrl;
        if (end - start).length() > 6.0 {
            // Рамка: выбрать все свои отряды, хотя бы один солдат которых попал внутрь.
            let (lo, hi) = (start.min(end), start.max(end));
            if !add {
                self.selected.fill(false);
            }
            let units = self.battle.units();
            for (s, b) in self.battle.soldiers() {
                let u = &units[s.unit.index()];
                if u.side != PLAYER || !u.is_fighting() || self.selected[u.id.index()] {
                    continue;
                }
                if let Some(px) = self.camera.project(to_world(b.pos, 1.0), size)
                    && px.cmpge(lo).all()
                    && px.cmple(hi).all()
                {
                    self.selected[u.id.index()] = true;
                }
            }
            return;
        }
        if let Some(id) = self.card_at(end, size) {
            self.select(id, add);
            return;
        }
        match self.unit_at(end, size) {
            Some(id) if self.battle.units()[id.index()].side == PLAYER => self.select(id, add),
            Some(_) => {}
            None => {
                if !add {
                    self.selected.fill(false);
                }
            }
        }
    }

    fn select(&mut self, id: UnitId, add: bool) {
        if !self.battle.units()[id.index()].is_fighting() {
            return;
        }
        if add {
            self.selected[id.index()] = !self.selected[id.index()];
        } else {
            self.selected.fill(false);
            self.selected[id.index()] = true;
        }
    }

    fn right_click(&mut self, start: V2, ground_start: Option<Vec2>, end: V2, size: V2) {
        let ids = self.selected_ids();
        if ids.is_empty() {
            return;
        }
        let double = self
            .input
            .last_right
            .is_some_and(|(t, p)| self.clock - t < DOUBLE_CLICK && (p - end).length() < 12.0);
        self.input.last_right = Some((self.clock, end));
        let run = double;
        let Some(ground_end) = self.ground_at(end, size) else {
            return;
        };

        // Щелчок по врагу — атака.
        if (end - start).length() < 6.0
            && let Some(target) = self.unit_at(end, size)
        {
            let t = &self.battle.units()[target.index()];
            if t.side != PLAYER {
                for &unit in &ids {
                    self.pending.push(Command::Attack { unit, target, run });
                }
                self.markers.push(Marker {
                    pos: t.center,
                    born: self.clock,
                    color: [0.9, 0.15, 0.1, 0.6],
                });
                return;
            }
        }

        let units: Vec<&Regiment> = ids
            .iter()
            .map(|id| &self.battle.units()[id.index()])
            .collect();
        let group_center =
            units.iter().fold(Vec2::ZERO, |a, u| a + u.center) * (1.0 / units.len() as f32);
        let line = ground_start.map(|g| ground_end - g);
        let cmds = match (ground_start, line) {
            // Протянули с зажатой кнопкой — это линия фронта.
            (Some(g), Some(l)) if l.length() > 4.0 => {
                let along = l.normalize_or(Vec2::new(1.0, 0.0));
                // Лицом от камеры: из двух перпендикуляров берём тот, что «вперёд».
                let (fwd, _) = self.camera.ground_axes();
                let cam_fwd = to_field(fwd);
                let mut facing = Vec2::new(-along.y, along.x);
                if facing.dot(cam_fwd) < 0.0 {
                    facing = -facing;
                }
                formation(&units, g + l * 0.5, facing, Some(l.length()), run)
            }
            _ => {
                let to = ground_end - group_center;
                let facing = if to.length() > 5.0 {
                    to.normalize_or(units[0].facing)
                } else {
                    units[0].facing
                };
                formation(&units, ground_end, facing, None, run)
            }
        };
        self.pending.extend(cmds);
        self.markers.push(Marker {
            pos: ground_end,
            born: self.clock,
            color: [0.2, 0.9, 0.3, 0.6],
        });
    }

    /// Собирает кадр: модели, метки на земле и интерфейс.
    pub fn build_frame(&mut self, alpha: f32, size: V2) -> (FrameConstants, [f32; 4]) {
        for list in &mut self.meshes {
            list.clear();
        }
        self.decals.clear();
        self.ui.clear();
        self.build_world(alpha, size);
        self.build_hud(size);

        let eye = self.camera.eye();
        let constants = FrameConstants {
            view_proj: self
                .camera
                .view_proj(size.x / size.y.max(1.0))
                .to_cols_array_2d(),
            sun: [0.45, 0.8, 0.35, self.battle.config().field_half_size],
            eye: [eye.x, eye.y, eye.z, 250.0],
            fog: [SKY[0], SKY[1], SKY[2], 900.0],
        };
        (constants, SKY)
    }

    pub fn frame_lists(&self) -> (&[Vec<Instance>; MESH_COUNT], &[Instance], &[UiRect]) {
        (&self.meshes, &self.decals, &self.ui)
    }

    fn build_world(&mut self, alpha: f32, size: V2) {
        let battle = &self.battle;
        let units = battle.units();
        let hovered = self.unit_at(self.input.cursor, size);
        let time = battle.time_seconds() + alpha * battle.config().tick_seconds;

        for (s, b) in battle.soldiers() {
            let u = &units[s.unit.index()];
            let [c1, c2] = self.colors[usize::from(s.side)];
            let pos = b.prev.lerp(b.pos, alpha);
            let walk = if b.speed.0 > 0.2 {
                time * b.speed.0 * 2.8 + f32::from(s.slot) * 0.77
            } else {
                0.0
            };
            let strike = if s.since_strike < 10 {
                (f32::from(s.since_strike) / 10.0 * std::f32::consts::PI).sin()
            } else {
                0.0
            };
            let mesh = if u.stats.cavalry {
                MeshId::Horseman
            } else if u.stats.missile.is_some() {
                MeshId::Crossbowman
            } else if u.stats.spearmen {
                MeshId::Spearman
            } else {
                MeshId::Levy
            };
            self.meshes[mesh as usize].push(Instance {
                pos_scale: world4(pos, 0.0, 1.0),
                dir_anim: [b.facing.x, -b.facing.y, walk, strike],
                color1: c1,
                color2: c2,
            });
            // Под каждым солдатом — тень, у выделенных — яркий круг.
            let r = u.stats.radius * 1.3;
            let decal_color = if self.selected[u.id.index()] {
                [1.0, 0.9, 0.3, 0.55]
            } else if hovered == Some(u.id) {
                [1.0, 1.0, 1.0, 0.35]
            } else {
                [0.0, 0.0, 0.0, 0.28]
            };
            self.decals.push(flat(pos, r, decal_color));
        }

        for p in battle.projectiles() {
            let t = p.progress(alpha);
            let ground = p.from.lerp(p.to, t);
            let dist = p.from.distance(p.to);
            let h = 1.5 * (1.0 - t) + 0.3 * t + 4.0 * dist * 0.08 * t * (1.0 - t);
            let dir = (p.to - p.from).normalize_or(Vec2::NORTH);
            self.meshes[MeshId::Arrow as usize].push(Instance {
                pos_scale: world4(ground, h, 1.0),
                dir_anim: [dir.x, -dir.y, 0.0, 0.0],
                color1: [1.0; 4],
                color2: [1.0; 4],
            });
        }

        for u in units.iter().filter(|u| u.is_active() && u.alive > 0) {
            let [mut c1, mut c2] = self.colors[usize::from(u.side)];
            if u.routing {
                // Бегущий отряд «выбрасывает белый флаг».
                c1 = [0.9, 0.9, 0.9, 1.0];
                c2 = c1;
            }
            let selected = self.selected[u.id.index()];
            let back = u.center - u.facing * (u.depth() * 0.5 + 1.5);
            self.meshes[MeshId::Banner as usize].push(Instance {
                pos_scale: world4(back, 0.0, if selected { 1.5 } else { 1.15 }),
                dir_anim: [u.facing.x, -u.facing.y, 0.0, 0.0],
                color1: c1,
                color2: c2,
            });
            // Куда идёт выделенный отряд: призрачный строй на земле.
            if selected {
                match u.order {
                    Order::Move { dest, facing, .. } if u.state == UnitState::Moving => {
                        let mut ghost = u.clone();
                        ghost.facing = facing;
                        for slot in 0..u.alive {
                            let p = dest + ghost.slot_offset(slot);
                            self.decals.push(flat(p, 0.3, [0.3, 1.0, 0.4, 0.35]));
                        }
                    }
                    Order::Attack { target, .. } => {
                        let t = &units[target.index()];
                        if t.is_active() {
                            self.decals.push(flat(t.center, 4.0, [1.0, 0.2, 0.1, 0.18]));
                        }
                    }
                    _ => {}
                }
            }
        }

        for m in &self.markers {
            let age = (self.clock - m.born) / 0.8;
            let mut c = m.color;
            c[3] *= 1.0 - age;
            self.decals.push(flat(m.pos, 1.5 + age * 3.0, c));
        }
    }

    fn build_hud(&mut self, size: V2) {
        let scale = (size.y / 360.0).floor().clamp(2.0, 4.0);
        let cards = card_layout(&self.battle, size);
        let hovered_enemy = (self.selected.iter().any(|&s| s))
            .then(|| self.unit_at(self.input.cursor, size))
            .flatten()
            .filter(|id| self.battle.units()[id.index()].side != PLAYER);
        let mut hud = Hud {
            rects: &mut self.ui,
        };
        let battle = &self.battle;

        // Верх: силы сторон и время.
        let t = battle.time_seconds() as u32;
        let names = [
            faction_title(battle.faction(0)),
            faction_title(battle.faction(1)),
        ];
        let top = format!(
            "{} {}   {:02}:{:02}   {} {}",
            names[0],
            battle.alive(0),
            t / 60,
            t % 60,
            battle.alive(1),
            names[1]
        );
        let tw = hud::text_width(&top, scale);
        hud.rect(
            size.x * 0.5 - tw * 0.5 - 12.0,
            0.0,
            tw + 24.0,
            hud::text_height(scale) + 16.0,
            [0.05, 0.05, 0.08, 0.6],
        );
        hud.text_centered(size.x * 0.5, 8.0, &top, scale, WHITE);
        let mut status = Vec::new();
        if self.paused {
            status.push("ПАУЗА".to_string());
        }
        if self.speed != 1.0 {
            status.push(format!("СКОРОСТЬ ×{}", self.speed));
        }
        if self.demo {
            status.push("ДЕМО: ИИ ПРОТИВ ИИ".to_string());
        }
        if !status.is_empty() {
            hud.text_centered(
                size.x * 0.5,
                16.0 + hud::text_height(scale) + 6.0,
                &status.join("   "),
                scale,
                [1.0, 0.85, 0.3, 1.0],
            );
        }

        // Подсказка по управлению.
        if self.show_help {
            let s = (scale - 1.0).max(1.0);
            let lines = [
                "ЛКМ — выбрать отряд, рамка — несколько",
                "1-9 — отряд по номеру, Ctrl+A — все",
                "ПКМ по земле — идти, протянуть — строй",
                "ПКМ по врагу — атака",
                "двойной ПКМ — бегом",
                "WASD/стрелки — камера, Q/E — поворот",
                "колесо — приближение, СКМ — вращение",
                "Пробел — пауза, +/- — скорость",
                "Backspace — стоять, Home — к армии",
                "F1 — скрыть подсказку, Esc — выход",
            ];
            let lh = hud::text_height(s) + 4.0 * s / 2.0;
            let w = lines
                .iter()
                .map(|l| hud::text_width(l, s))
                .fold(0.0, f32::max);
            hud.rect(
                8.0,
                8.0,
                w + 16.0,
                lh * lines.len() as f32 + 12.0,
                [0.05, 0.05, 0.08, 0.55],
            );
            for (i, l) in lines.iter().enumerate() {
                hud.text(16.0, 14.0 + i as f32 * lh, l, s, [0.92, 0.92, 0.85, 1.0]);
            }
        }

        // Карточки отрядов игрока.
        for (n, (id, x, y, w, h)) in cards.into_iter().enumerate() {
            let u = &battle.units()[id.index()];
            let sel = self.selected[id.index()];
            let [c1, _] = self.colors[usize::from(PLAYER)];
            let bg = if u.is_fighting() {
                [0.08, 0.08, 0.1, 0.78]
            } else {
                [0.15, 0.04, 0.04, 0.7]
            };
            let s = (scale - 1.0).max(1.0);
            hud.rect(x, y, w, h, bg);
            hud.rect(x, y, w, 7.0 * s + 4.0, c1);
            if sel {
                hud.frame(
                    x - 2.0,
                    y - 2.0,
                    w + 4.0,
                    h + 4.0,
                    3.0,
                    [1.0, 0.85, 0.3, 1.0],
                );
            }
            let s = (scale - 1.0).max(1.0);
            let mut ty = y + 7.0 * s + 10.0;
            for line in hud::wrap(&u.name, 12).iter().take(2) {
                hud.text(x + 8.0, ty, line, s, WHITE);
                ty += hud::text_height(s) + 4.0;
            }
            let count = format!("{}/{}", u.alive, u.initial_count);
            hud.text(
                x + 8.0,
                y + h - 26.0 - hud::text_height(s),
                &count,
                s,
                [0.85, 0.85, 0.85, 1.0],
            );
            let (state, color) = state_label(u);
            hud.text(
                x + w - 8.0 - hud::text_width(state, s),
                y + h - 26.0 - hud::text_height(s),
                state,
                s,
                color,
            );
            // Полоска морали.
            let m = (u.morale / u.stats.morale).clamp(0.0, 1.0);
            hud.rect(x + 8.0, y + h - 16.0, w - 16.0, 8.0, [0.2, 0.2, 0.2, 1.0]);
            hud.rect(
                x + 8.0,
                y + h - 16.0,
                (w - 16.0) * m,
                8.0,
                [1.0 - m, 0.25 + 0.6 * m, 0.15, 1.0],
            );
            // Номер для выбора с клавиатуры.
            if n < 9 {
                hud.text(x + 4.0, y + 2.0, &(n + 1).to_string(), s, WHITE);
            }
        }

        // Рамка выделения.
        if let Some(start) = self.input.left_start {
            let (lo, hi) = (start.min(self.input.cursor), start.max(self.input.cursor));
            if (hi - lo).length() > 6.0 {
                hud.rect(lo.x, lo.y, hi.x - lo.x, hi.y - lo.y, [1.0, 1.0, 1.0, 0.08]);
                hud.frame(
                    lo.x,
                    lo.y,
                    hi.x - lo.x,
                    hi.y - lo.y,
                    1.0,
                    [1.0, 1.0, 1.0, 0.8],
                );
            }
        }
        if hovered_enemy.is_some() {
            let c = self.input.cursor;
            hud.text_shadow(
                c.x + 16.0,
                c.y + 8.0,
                "АТАКА",
                (scale - 1.0).max(1.0),
                [1.0, 0.35, 0.25, 1.0],
            );
        }

        // Итог битвы.
        if let Some(outcome) = battle.outcome() {
            let (title, color) = match outcome {
                Outcome::Victory { winner } if winner == PLAYER => {
                    ("ПОБЕДА!", [1.0, 0.85, 0.3, 1.0])
                }
                Outcome::Victory { .. } => ("ПОРАЖЕНИЕ", [0.95, 0.3, 0.25, 1.0]),
                Outcome::Draw => ("НИЧЬЯ", WHITE),
            };
            let big = scale * 3.0;
            let cy = size.y * 0.3;
            hud.rect(
                0.0,
                cy - 20.0,
                size.x,
                hud::text_height(big) + hud::text_height(scale) * 3.0 + 60.0,
                [0.0, 0.0, 0.0, 0.5],
            );
            hud.text_centered(size.x * 0.5, cy, title, big, color);
            let stats = format!(
                "Потери: {} убито, {} бежало.  Враг: {} убито, {} бежало.",
                battle.killed[0], battle.fled[0], battle.killed[1], battle.fled[1]
            );
            let y2 = cy + hud::text_height(big) + 18.0;
            hud.text_centered(size.x * 0.5, y2, &stats, (scale - 1.0).max(1.0), WHITE);
            hud.text_centered(
                size.x * 0.5,
                y2 + hud::text_height(scale) + 10.0,
                "Enter — новая битва, Esc — выход",
                (scale - 1.0).max(1.0),
                [0.9, 0.9, 0.8, 1.0],
            );
        }
    }

    fn card_at(&self, p: V2, size: V2) -> Option<UnitId> {
        card_layout(&self.battle, size)
            .into_iter()
            .find(|&(_, x, y, w, h)| p.x >= x && p.x <= x + w && p.y >= y && p.y <= y + h)
            .map(|(id, ..)| id)
    }
}

/// Расстановка битвы-прототипа: две армии друг напротив друга на ровном поле.
pub fn default_setup(seed: u64) -> BattleSetup {
    let line = |units: &[&str], y: f32, facing: Vec2, gap: f32| -> Vec<UnitSetup> {
        let n = units.len() as f32;
        units
            .iter()
            .enumerate()
            .map(|(i, u)| UnitSetup {
                unit: (*u).into(),
                position: Vec2::new((i as f32 - (n - 1.0) * 0.5) * gap, y),
                facing,
            })
            .collect()
    };
    let mut player = line(
        &[
            "Village Levy",
            "Shield Wall Spearmen",
            "Shield Wall Spearmen",
            "Village Levy",
        ],
        -100.0,
        Vec2::NORTH,
        26.0,
    );
    player.extend(line(
        &["Crossbow Militia", "Crossbow Militia"],
        -125.0,
        Vec2::NORTH,
        60.0,
    ));
    let mut enemy = line(
        &[
            "Village Levy",
            "Village Levy",
            "Shield Wall Spearmen",
            "Village Levy",
            "Village Levy",
        ],
        100.0,
        -Vec2::NORTH,
        24.0,
    );
    enemy.extend(line(
        &["Crossbow Militia", "Crossbow Militia"],
        125.0,
        -Vec2::NORTH,
        50.0,
    ));
    for x in [-95.0, 95.0] {
        enemy.push(UnitSetup {
            unit: "Steppe Horse Archers".into(),
            position: Vec2::new(x, 110.0),
            facing: -Vec2::NORTH,
        });
    }
    BattleSetup {
        seed,
        armies: [
            ArmySetup {
                faction: "northmark".into(),
                units: player,
            },
            ArmySetup {
                faction: "southreach".into(),
                units: enemy,
            },
        ],
    }
}

fn new_battle(data: &GameData, seed: u64) -> Result<Battle> {
    Battle::new(data, &default_setup(seed), BattleConfig::default())
        .context("не удалось расставить армии")
}

/// Ищет папку `data/`: рядом с программой, в текущей папке или выше
/// (так находится и при запуске через `cargo run`).
pub fn find_data_dir(explicit: Option<&Path>) -> Result<PathBuf> {
    if let Some(p) = explicit {
        return Ok(p.to_path_buf());
    }
    let mut candidates = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        candidates.extend(exe.ancestors().skip(1).take(4).map(|d| d.join("data")));
    }
    if let Ok(cwd) = std::env::current_dir() {
        candidates.extend(cwd.ancestors().take(4).map(|d| d.join("data")));
    }
    candidates
        .into_iter()
        .find(|d| d.join(rm_data::UNITS_FILE).is_file())
        .context("не найдена папка data/ с export_descr_unit.txt; укажите её параметром --data")
}

/// Карточки отрядов игрока внизу экрана: (отряд, x, y, ширина, высота).
fn card_layout(battle: &Battle, size: V2) -> Vec<(UnitId, f32, f32, f32, f32)> {
    let scale = (size.y / 360.0).floor().clamp(2.0, 4.0);
    let s = (scale - 1.0).max(1.0);
    let w = 12.0 * 6.0 * s + 16.0;
    let h = 7.0 * s + 10.0 + 2.0 * (hud::text_height(s) + 4.0) + hud::text_height(s) + 34.0;
    let ids: Vec<UnitId> = battle
        .units()
        .iter()
        .filter(|u| u.side == PLAYER)
        .map(|u| u.id)
        .collect();
    let gap = 8.0;
    let total = ids.len() as f32 * (w + gap) - gap;
    let x0 = (size.x - total) * 0.5;
    ids.into_iter()
        .enumerate()
        .map(|(i, id)| (id, x0 + i as f32 * (w + gap), size.y - h - 10.0, w, h))
        .collect()
}

fn state_label(u: &Regiment) -> (&'static str, [f32; 4]) {
    match u.state {
        _ if !u.is_active() => ("РАЗБИТ", [0.6, 0.6, 0.6, 1.0]),
        _ if u.routing => ("БЕЖИТ", [1.0, 0.35, 0.25, 1.0]),
        UnitState::Melee => ("БОЙ", [1.0, 0.6, 0.2, 1.0]),
        UnitState::Firing => ("ОГОНЬ", [1.0, 0.85, 0.4, 1.0]),
        UnitState::Moving => ("МАРШ", [0.6, 0.85, 1.0, 1.0]),
        _ => ("ГОТОВ", [0.6, 1.0, 0.6, 1.0]),
    }
}

/// Красивое имя фракции из внутреннего: `southreach` → `SOUTHREACH`.
fn faction_title(name: &str) -> String {
    name.replace('_', " ").to_uppercase()
}

fn side_center(battle: &Battle, side: Side) -> Vec2 {
    let list: Vec<&Regiment> = battle.units().iter().filter(|u| u.side == side).collect();
    list.iter().fold(Vec2::ZERO, |a, u| a + u.center) * (1.0 / list.len().max(1) as f32)
}

/// Выстраивает отряды в линию с центром `center` лицом в `facing`.
/// Если задана ширина линии `width`, строй растягивается под неё.
fn formation(
    units: &[&Regiment],
    center: Vec2,
    facing: Vec2,
    width: Option<f32>,
    run: bool,
) -> Vec<Command> {
    const GAP: f32 = 4.0;
    let right = facing.right();
    let mut order: Vec<&Regiment> = units.to_vec();
    // Кто левее сейчас, тот левее и в новом строю: отряды не перебегают друг через друга.
    order.sort_by(|a, b| {
        a.center
            .dot(right)
            .total_cmp(&b.center.dot(right))
            .then(a.id.cmp(&b.id))
    });
    let natural: f32 =
        order.iter().map(|u| u.front_width()).sum::<f32>() + GAP * (order.len() as f32 - 1.0);
    let k = width.map_or(1.0, |w| (w / natural.max(1.0)).clamp(0.3, 4.0));
    let fronts: Vec<f32> = order.iter().map(|u| u.front_width() * k).collect();
    let total: f32 = fronts.iter().sum::<f32>() + GAP * (order.len() as f32 - 1.0);
    let mut x = -total * 0.5;
    order
        .iter()
        .zip(fronts)
        .map(|(u, front)| {
            let dest = center + right * (x + front * 0.5);
            x += front + GAP;
            Command::Move {
                unit: u.id,
                dest,
                facing: Some(facing),
                width: width.map(|_| ((front / u.stats.spacing_side).round() as u32).max(1)),
                run,
            }
        })
        .collect()
}

fn digit(code: KeyCode) -> Option<usize> {
    let keys = [
        KeyCode::Digit1,
        KeyCode::Digit2,
        KeyCode::Digit3,
        KeyCode::Digit4,
        KeyCode::Digit5,
        KeyCode::Digit6,
        KeyCode::Digit7,
        KeyCode::Digit8,
        KeyCode::Digit9,
    ];
    keys.iter().position(|&k| k == code)
}

/// Позиция и масштаб экземпляра в мире.
fn world4(p: Vec2, height: f32, scale: f32) -> [f32; 4] {
    let w = to_world(p, height);
    [w.x, w.y, w.z, scale]
}

/// Плоский круг на земле.
fn flat(p: Vec2, radius: f32, color: [f32; 4]) -> Instance {
    Instance {
        pos_scale: world4(p, 0.03, radius),
        dir_anim: [0.0, 1.0, 0.0, 0.0],
        color1: color,
        color2: color,
    }
}

/// Цвет из файла данных (sRGB, 0..255) в линейный для шейдеров.
fn srgb(r: u8, g: u8, b: u8) -> [f32; 4] {
    let lin = |c: u8| (f32::from(c) / 255.0).powf(2.2);
    [lin(r), lin(g), lin(b), 1.0]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data_dir() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data")
    }

    #[test]
    fn default_battle_loads_from_data() {
        let game = Game::new(&data_dir(), 1, false).unwrap();
        assert!(game.battle().units().len() >= 10);
        assert!(game.battle().units().iter().any(|u| u.side == 0));
    }

    #[test]
    fn formation_lines_units_up_left_to_right() {
        let game = Game::new(&data_dir(), 1, false).unwrap();
        let units: Vec<&Regiment> = game.player_units().take(3).collect();
        let cmds = formation(&units, Vec2::new(0.0, 0.0), Vec2::NORTH, None, false);
        let xs: Vec<f32> = cmds
            .iter()
            .map(|c| match c {
                Command::Move { dest, .. } => dest.x,
                _ => panic!("ждали приказ идти"),
            })
            .collect();
        assert!(xs.windows(2).all(|w| w[0] < w[1]), "{xs:?}");
        assert!((xs.iter().sum::<f32>() / 3.0).abs() < 10.0);
    }

    #[test]
    fn demo_battle_runs_headless() {
        let mut game = Game::new(&data_dir(), 3, true).unwrap();
        for _ in 0..20 * 30 {
            game.tick();
        }
        assert!(
            game.battle().killed.iter().sum::<u32>() > 0,
            "за 30 секунд никто не погиб"
        );
    }
}
