//! Отряд (regiment): статы из данных, приказ, строй, мораль.
//!
//! Солдаты живут в ECS как отдельные сущности, а всё, что относится к
//! отряду целиком, хранится здесь. Отрядов в битве десятки, поэтому
//! обычный `Vec` подходит лучше, чем ECS.

use rm_data::unit::Weapon;
use rm_data::{Unit, UnitCategory, UnitClass};

use crate::config::BattleConfig;
use crate::math::Vec2;

/// Номер отряда в битве (индекс в `Battle::units`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct UnitId(pub u16);

impl UnitId {
    pub fn index(self) -> usize {
        usize::from(self.0)
    }
}

/// Сторона: 0 — игрок (юг), 1 — противник (север).
pub type Side = u8;

/// Приказ отряду. Меняется только командами (`crate::Command`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Order {
    /// Стоять в строю там, где стоит. Солдаты сами отбиваются от подошедших врагов.
    Hold,
    /// Перестроиться в точке `dest` лицом в сторону `facing`.
    Move { dest: Vec2, facing: Vec2, run: bool },
    /// Атаковать отряд `target`: стрелки стреляют, остальные идут в рукопашную.
    Attack { target: UnitId, run: bool },
}

/// Что отряд делает прямо сейчас (для интерфейса и ИИ).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnitState {
    Ready,
    Moving,
    Firing,
    Melee,
    Routing,
    /// Отряд уничтожен или убежал с поля.
    Gone,
}

/// Метательное оружие отряда.
#[derive(Debug, Clone, PartialEq)]
pub struct MissileStats {
    pub attack: i32,
    pub range: f32,
    pub ammo: u16,
    pub reload_ticks: u32,
}

/// Статы отряда, нужные бою. Собираются из записи `export_descr_unit.txt`.
#[derive(Debug, Clone, PartialEq)]
pub struct UnitStats {
    pub cavalry: bool,
    pub spearmen: bool,
    pub walk_speed: f32,
    pub run_speed: f32,
    pub radius: f32,
    pub mass: f32,
    pub hit_points: u16,
    /// Атака в рукопашной (у стрелков — второе оружие, если оно есть).
    pub melee_attack: i32,
    pub charge_bonus: i32,
    /// Задержка между ударами, шагов.
    pub melee_delay_ticks: u32,
    pub armour: i32,
    pub defence_skill: i32,
    pub shield: i32,
    pub missile: Option<MissileStats>,
    /// Расстояние между солдатами в строю, метры: по фронту и в глубину.
    pub spacing_side: f32,
    pub spacing_back: f32,
    pub ranks: u32,
    pub morale: f32,
    /// Множитель потерь морали: недисциплинированные пугаются сильнее.
    pub morale_sensitivity: f32,
}

impl UnitStats {
    pub fn from_data(unit: &Unit, cfg: &BattleConfig) -> Self {
        let cavalry = unit.category == UnitCategory::Cavalry;
        let pri = &unit.primary_weapon;
        let is_missile = |w: &Weapon| w.weapon_type == "missile" || w.weapon_type == "thrown";
        let missile = (is_missile(pri) && pri.ammo > 0 && pri.range > 0).then(|| {
            let reload = match pri.tech_type.as_str() {
                "crossbow" => cfg.reload_crossbow,
                "archery" => cfg.reload_bow,
                _ => cfg.reload_other,
            };
            MissileStats {
                attack: pri.attack as i32,
                range: pri.range as f32,
                ammo: pri.ammo.min(u32::from(u16::MAX)) as u16,
                reload_ticks: cfg.ticks(reload),
            }
        });
        // Оружие для рукопашной: у стрелков это второе оружие, если оно есть.
        let melee = if is_missile(pri) {
            unit.secondary_weapon.as_ref().unwrap_or(pri)
        } else {
            pri
        };
        let melee_attack = if is_missile(melee) {
            1
        } else {
            melee.attack as i32
        };
        let (spacing_side, spacing_back, ranks) = match &unit.formation {
            Some(f) => (f.tight_side, f.tight_back, f.ranks.max(1)),
            None => (1.2, 1.2, 4),
        };
        let radius = if cavalry {
            cfg.cavalry_radius
        } else {
            cfg.infantry_radius
        };
        let morale_sensitivity = match unit.mental.discipline.as_str() {
            "low" => 1.4,
            "disciplined" => 0.7,
            "berserker" => 0.0,
            _ => 1.0,
        };
        Self {
            cavalry,
            spearmen: unit.class == UnitClass::Spearmen,
            walk_speed: if cavalry {
                cfg.cavalry_walk
            } else {
                cfg.infantry_walk
            },
            run_speed: if cavalry {
                cfg.cavalry_run
            } else {
                cfg.infantry_run
            },
            radius,
            mass: unit.soldier.mass.max(0.1),
            hit_points: unit.health.soldier.clamp(1, 100) as u16,
            melee_attack,
            charge_bonus: melee.charge_bonus as i32,
            // `min_delay` в десятых долях секунды.
            melee_delay_ticks: cfg.ticks(melee.min_delay as f32 / 10.0),
            armour: unit.armour.armour as i32,
            defence_skill: unit.armour.defence_skill as i32,
            shield: unit.armour.shield as i32,
            missile,
            // Строй не плотнее, чем позволяют цилиндры солдат.
            spacing_side: spacing_side.max(radius * 2.2),
            spacing_back: spacing_back.max(radius * 2.2),
            ranks,
            morale: cfg.morale_base + cfg.morale_per_point * unit.mental.morale as f32,
            morale_sensitivity,
        }
    }
}

/// Состояние отряда в битве.
#[derive(Debug, Clone, PartialEq)]
pub struct Regiment {
    pub id: UnitId,
    pub side: Side,
    /// Внутреннее имя юнита (`type` в данных).
    pub name: String,
    pub stats: UnitStats,
    pub initial_count: u32,
    pub alive: u32,
    /// Сколько выстрелов осталось у всего отряда.
    pub ammo: u32,
    pub order: Order,
    pub state: UnitState,
    /// Точка, вокруг которой строится отряд, и направление фронта.
    pub anchor: Vec2,
    pub facing: Vec2,
    /// Число солдат в первой шеренге.
    pub width: u32,
    /// Средняя позиция живых солдат (пересчитывается каждый шаг).
    pub center: Vec2,
    pub morale: f32,
    pub routing: bool,
    pub rallied_once: bool,
    /// Шагов бонуса натиска ещё осталось.
    pub charge_ticks: u32,
    /// Пройдено бегом с момента приказа атаковать (для натиска).
    pub run_distance: f32,
    /// Отряд был в рукопашной на этом шаге.
    pub in_melee: bool,
    /// Солдатам нужно заново раздать места в строю (кто-то погиб).
    pub(crate) slots_dirty: bool,
    pub(crate) casualties_tick: u32,
    pub(crate) rear_hits_tick: u32,
}

impl Regiment {
    pub fn is_active(&self) -> bool {
        self.state != UnitState::Gone
    }

    /// Можно ли этим отрядом управлять и атаковать его как цель.
    pub fn is_fighting(&self) -> bool {
        self.is_active() && !self.routing
    }

    pub fn speed(&self, run: bool) -> f32 {
        if run {
            self.stats.run_speed
        } else {
            self.stats.walk_speed
        }
    }

    /// Ширина фронта при текущем числе солдат.
    pub fn front_width(&self) -> f32 {
        self.width.min(self.alive.max(1)) as f32 * self.stats.spacing_side
    }

    /// Смещение места `slot` от точки построения, в координатах поля.
    pub fn slot_offset(&self, slot: u32) -> Vec2 {
        let width = self.width.min(self.alive.max(1)).max(1);
        let row = slot / width;
        let col = slot % width;
        // Последняя неполная шеренга выравнивается по центру.
        let in_row = if row == (self.alive.max(1) - 1) / width {
            (self.alive - row * width).clamp(1, width)
        } else {
            width
        };
        let x = (col as f32 - (in_row as f32 - 1.0) * 0.5) * self.stats.spacing_side;
        let y = -(row as f32) * self.stats.spacing_back;
        self.facing.right() * x + self.facing * y
    }

    /// Глубина строя, метры.
    pub fn depth(&self) -> f32 {
        let width = self.width.min(self.alive.max(1)).max(1);
        (self.alive.max(1).div_ceil(width)) as f32 * self.stats.spacing_back
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn regiment(alive: u32, width: u32) -> Regiment {
        let stats = UnitStats {
            cavalry: false,
            spearmen: false,
            walk_speed: 2.0,
            run_speed: 4.0,
            radius: 0.4,
            mass: 1.0,
            hit_points: 1,
            melee_attack: 3,
            charge_bonus: 1,
            melee_delay_ticks: 50,
            armour: 0,
            defence_skill: 0,
            shield: 0,
            missile: None,
            spacing_side: 1.0,
            spacing_back: 2.0,
            ranks: 2,
            morale: 50.0,
            morale_sensitivity: 1.0,
        };
        Regiment {
            id: UnitId(0),
            side: 0,
            name: "test".into(),
            stats,
            initial_count: alive,
            alive,
            ammo: 0,
            order: Order::Hold,
            state: UnitState::Ready,
            anchor: Vec2::ZERO,
            facing: Vec2::NORTH,
            width,
            center: Vec2::ZERO,
            morale: 50.0,
            routing: false,
            rallied_once: false,
            charge_ticks: 0,
            run_distance: 0.0,
            in_melee: false,
            slots_dirty: false,
            casualties_tick: 0,
            rear_hits_tick: 0,
        }
    }

    #[test]
    fn slots_form_centered_ranks() {
        let r = regiment(5, 3);
        // Первая шеренга: -1, 0, +1 по оси x (фронт смотрит на север).
        assert_eq!(r.slot_offset(0), Vec2::new(-1.0, 0.0));
        assert_eq!(r.slot_offset(2), Vec2::new(1.0, 0.0));
        // Вторая шеренга из двух солдат выровнена по центру и стоит сзади (юг).
        assert_eq!(r.slot_offset(3), Vec2::new(-0.5, -2.0));
        assert_eq!(r.slot_offset(4), Vec2::new(0.5, -2.0));
        assert_eq!(r.depth(), 4.0);
    }
}
