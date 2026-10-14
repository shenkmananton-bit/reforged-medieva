//! # rm_battle_ai — тактический ИИ битвы
//!
//! Иерархия «генерал → отряд» (хак H-21): решения принимаются для отрядов,
//! а не для каждого солдата. ИИ ничего не меняет в битве сам: он читает её
//! состояние и возвращает список [`Command`], ровно как игрок мышью.
//! Поэтому ИИ детерминирован и попадает в реплей как обычные команды.
//!
//! Поведение (прототип):
//! 1. **Сближение.** Пока враг далеко, вся армия шагом идёт ему навстречу,
//!    сохраняя построение.
//! 2. **Бой.** Стрелки бьют ближайший отряд в радиусе; конные стрелки
//!    отходят, если к ним подбирается враг. Конница охотится за стрелками. Пехота атакует
//!    ближайший боеспособный отряд и переходит на бег перед ударом.
//!    Бегущих врагов не преследуем — их добьют те, кто рядом.
//!
//! Подробно: `docs/battle/ai.md`.

use rm_battle::{Battle, Command, Order, Regiment, Side, UnitId, UnitState, Vec2};

/// Настройки ИИ. Это не статы отрядов, а «характер» генерала.
#[derive(Debug, Clone, PartialEq)]
pub struct AiConfig {
    /// Как часто генерал думает, шагов симуляции.
    pub think_every: u64,
    /// Ближе этого расстояния до врага армия перестаёт маршировать строем.
    pub engage_range: f32,
    /// Шаг марша за одно решение, метры.
    pub march_step: f32,
    /// С какого расстояния до цели отряд переходит на бег.
    pub charge_range: f32,
    /// Стрелки отходят, если вражеская пехота ближе этого.
    pub skirmish_range: f32,
    /// Новая цель выбирается, только если она ближе текущей в это число раз.
    pub retarget_ratio: f32,
}

impl Default for AiConfig {
    fn default() -> Self {
        Self {
            think_every: 10,
            engage_range: 130.0,
            march_step: 25.0,
            charge_range: 45.0,
            skirmish_range: 30.0,
            retarget_ratio: 0.6,
        }
    }
}

/// Генерал одной стороны.
pub struct BattleAi {
    side: Side,
    cfg: AiConfig,
    engaged: bool,
}

impl BattleAi {
    pub fn new(side: Side) -> Self {
        Self::with_config(side, AiConfig::default())
    }

    pub fn with_config(side: Side, cfg: AiConfig) -> Self {
        Self {
            side,
            cfg,
            engaged: false,
        }
    }

    pub fn side(&self) -> Side {
        self.side
    }

    /// Решения на этот шаг. Генерал думает не каждый шаг, а раз в `think_every`.
    pub fn think(&mut self, battle: &Battle) -> Vec<Command> {
        if battle.outcome().is_some() || !battle.tick().is_multiple_of(self.cfg.think_every) {
            return Vec::new();
        }
        let units = battle.units();
        let mine: Vec<&Regiment> = units
            .iter()
            .filter(|u| u.side == self.side && u.is_fighting())
            .collect();
        let enemies: Vec<&Regiment> = units
            .iter()
            .filter(|u| u.side != self.side && u.is_fighting())
            .collect();
        if mine.is_empty() || enemies.is_empty() {
            return Vec::new();
        }

        let closest = mine
            .iter()
            .flat_map(|m| enemies.iter().map(|e| m.center.distance(e.center)))
            .fold(f32::MAX, f32::min);
        if closest < self.cfg.engage_range || mine.iter().any(|u| u.in_melee) {
            self.engaged = true;
        }

        let mut commands = Vec::new();
        if !self.engaged {
            // Марш: все отряды делают одинаковый шаг к центру вражеской армии.
            let enemy_center = centroid(&enemies);
            let my_center = centroid(&mine);
            let dir = (enemy_center - my_center).normalize_or(Vec2::NORTH);
            for u in &mine {
                commands.push(Command::Move {
                    unit: u.id,
                    dest: u.center + dir * self.cfg.march_step,
                    facing: Some(dir),
                    width: None,
                    run: false,
                });
            }
            return commands;
        }

        for u in &mine {
            if let Some(c) = self.unit_decision(u, &enemies) {
                commands.push(c);
            }
        }
        commands
    }

    fn unit_decision(&self, u: &Regiment, enemies: &[&Regiment]) -> Option<Command> {
        // В рукопашной отряд сам рубится с теми, кто рядом; не дёргаем его.
        if u.in_melee || u.state == UnitState::Melee {
            return None;
        }
        let current = match u.order {
            Order::Attack { target, .. } => enemies.iter().find(|e| e.id == target).copied(),
            _ => None,
        };
        let dist = |e: &Regiment| e.center.distance(u.center);

        if let Some(m) = &u.stats.missile
            && u.ammo > 0
        {
            // Конные стрелки отходят от подбегающих врагов, остальные стрелки стоят.
            if u.stats.cavalry
                && let Some(threat) = enemies
                    .iter()
                    .filter(|e| e.stats.missile.is_none() || e.ammo == 0)
                    .find(|e| dist(e) < self.cfg.skirmish_range)
            {
                let away = (u.center - threat.center).normalize_or(-u.facing);
                return Some(Command::Move {
                    unit: u.id,
                    dest: u.center + away * self.cfg.skirmish_range,
                    facing: Some(-away),
                    width: None,
                    run: true,
                });
            }
            if current.is_some_and(|c| dist(c) <= m.range) {
                return None;
            }
            let target = nearest(u, enemies.iter().copied())?;
            return attack(u, target.id, false);
        }

        // Конница сначала ищет стрелков поблизости.
        let preferred = if u.stats.cavalry {
            nearest(
                u,
                enemies
                    .iter()
                    .copied()
                    .filter(|e| e.stats.missile.is_some() && dist(e) < 150.0),
            )
        } else {
            None
        };
        let best = preferred.or_else(|| nearest(u, enemies.iter().copied()))?;
        let target = match current {
            Some(c) if dist(best) > dist(c) * self.cfg.retarget_ratio => c,
            _ => best,
        };
        let run = dist(target) < self.cfg.charge_range;
        attack(u, target.id, run)
    }
}

/// Команда атаки, если она что-то меняет.
fn attack(u: &Regiment, target: UnitId, run: bool) -> Option<Command> {
    match u.order {
        Order::Attack { target: t, run: r } if t == target && r == run => None,
        _ => Some(Command::Attack {
            unit: u.id,
            target,
            run,
        }),
    }
}

fn nearest<'a>(u: &Regiment, list: impl Iterator<Item = &'a Regiment>) -> Option<&'a Regiment> {
    list.min_by(|a, b| {
        a.center
            .distance_sq(u.center)
            .total_cmp(&b.center.distance_sq(u.center))
            .then(a.id.cmp(&b.id))
    })
}

fn centroid(list: &[&Regiment]) -> Vec2 {
    let sum = list.iter().fold(Vec2::ZERO, |acc, u| acc + u.center);
    sum * (1.0 / list.len().max(1) as f32)
}

#[cfg(test)]
mod tests;
