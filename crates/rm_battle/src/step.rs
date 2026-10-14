//! Один шаг симуляции боя (50 мс игрового времени).
//!
//! Порядок внутри шага всегда один и тот же:
//! 1. применить команды игрока и ИИ;
//! 2. собрать солдат из ECS в плотные массивы и обновить отряды (центр, места в строю);
//! 3. решить за каждый отряд, что он делает (идёт, стреляет, рубится, бежит);
//! 4. решить за каждого солдата, куда ему шагнуть и кого ударить;
//! 5. мягко растолкать перекрывающиеся цилиндры (хак H-08);
//! 6. посчитать удары и попадания снарядов;
//! 7. записать солдат обратно, убрать убитых и сбежавших;
//! 8. пересчитать мораль и проверить, не закончилась ли битва.
//!
//! Все случайные числа берутся из счётного ГСЧ `rm_core` по номеру шага
//! и номеру сущности, поэтому результат не зависит от порядка обхода.

use rm_core::Entity;
use rm_core::sim::{Rng, RngStream, random_u64};

use crate::grid::Grid;
use crate::math::Vec2;
use crate::regiment::{Order, UnitId, UnitState};
use crate::{Battle, Body, Command, Outcome, Projectile, Soldier, Speed};

/// Размер клетки сетки соседей, метры.
const CELL: f32 = 4.0;

/// Решение отряда на текущий шаг.
#[derive(Clone, Copy, Default)]
struct Plan {
    /// Радиус, в котором солдаты ищут врага для рукопашной (0 — не ищут).
    seek: f32,
    run: bool,
    flee: bool,
    fire_target: Option<UnitId>,
}

impl Battle {
    /// Выполняет один шаг симуляции с командами, которые пришли к этому шагу.
    pub fn step(&mut self, commands: &[Command]) {
        for c in commands {
            self.apply(c);
        }
        self.tick += 1;
        let dt = self.cfg.tick_seconds;

        // 2. Солдаты из ECS в плотные массивы (порядок обхода ECS стабилен).
        let mut ents: Vec<Entity> = Vec::new();
        let mut sol: Vec<Soldier> = Vec::new();
        let mut body: Vec<Body> = Vec::new();
        for (e, s, b) in self.world.query::<(Entity, &Soldier, &Body)>() {
            ents.push(e);
            sol.push(*s);
            body.push(*b);
        }
        self.update_regiments(&mut sol, &body);

        // Списки солдат по отрядам — для выбора цели стрелками.
        let mut members: Vec<Vec<u32>> = vec![Vec::new(); self.units.len()];
        for (i, s) in sol.iter().enumerate() {
            members[s.unit.index()].push(i as u32);
        }

        // 3. Решения отрядов.
        let mut ammo = vec![0u32; self.units.len()];
        for s in &sol {
            ammo[s.unit.index()] += u32::from(s.ammo);
        }
        for (u, &a) in self.units.iter_mut().zip(&ammo) {
            u.ammo = a;
        }
        let plans: Vec<Plan> = (0..self.units.len())
            .map(|u| self.plan_unit(u, ammo[u] > 0))
            .collect();

        // 4. Решения солдат.
        let positions: Vec<Vec2> = body.iter().map(|b| b.pos).collect();
        let grid = Grid::build(&positions, self.cfg.field_half_size, CELL);
        let mut new_pos = positions.clone();
        let mut strikes: Vec<(usize, usize)> = Vec::new();
        let mut shots: Vec<(usize, Vec2)> = Vec::new();
        for i in 0..sol.len() {
            let mut s = sol[i];
            let mut b = body[i];
            b.prev = b.pos;
            s.cooldown = s.cooldown.saturating_sub(1);
            s.since_strike = s.since_strike.saturating_add(1);
            s.fighting = false;
            let r = &self.units[s.unit.index()];
            let plan = plans[s.unit.index()];
            let mut rng =
                Rng::for_entity(self.seed, RngStream::COMBAT, self.tick, ents[i].to_bits());

            let mut velocity = Vec2::ZERO;
            if plan.flee {
                let home = if s.side == 0 {
                    -Vec2::NORTH
                } else {
                    Vec2::NORTH
                };
                let mut away = Vec2::ZERO;
                if let Some(j) = nearest_enemy(&grid, &positions, &sol, i, 10.0) {
                    away = (positions[i] - positions[j]).normalize_or(Vec2::ZERO);
                }
                let dir = (home + away).normalize_or(home);
                velocity = dir * r.stats.run_speed;
                b.facing = dir;
            } else if let Some(j) = (plan.seek > 0.0)
                .then(|| nearest_enemy(&grid, &positions, &sol, i, plan.seek))
                .flatten()
            {
                // Рукопашная: подойти к ближайшему врагу и бить.
                let other = &self.units[sol[j].unit.index()];
                let to = positions[j] - positions[i];
                let d = to.length();
                let dir = to.normalize_or(b.facing);
                b.facing = dir;
                s.fighting = true;
                let reach = self.cfg.melee_reach + r.stats.radius + other.stats.radius;
                if d > reach {
                    velocity = dir * r.speed(true).min((d - reach) / dt);
                } else if s.cooldown == 0 {
                    strikes.push((i, j));
                    let delay = r.stats.melee_delay_ticks;
                    s.cooldown = (delay + rng.below(delay / 2 + 1)).min(u32::from(u16::MAX)) as u16;
                    s.since_strike = 0;
                }
            } else {
                // Своё место в строю.
                let target = r.anchor + r.slot_offset(u32::from(s.slot));
                let to = target - positions[i];
                let d = to.length();
                if d > 0.15 {
                    let dir = to.normalize_or(b.facing);
                    // Отставшие догоняют чуть быстрее, у цели плавно тормозят.
                    let speed = r.speed(plan.run) * if d > 4.0 { 1.15 } else { 1.0 };
                    velocity = dir * speed.min(d / dt * 0.5);
                }
                b.facing = if d > 1.0 {
                    to.normalize_or(r.facing)
                } else {
                    r.facing
                };
                // Стрельба по цели отряда.
                if let Some(t) = plan.fire_target
                    && s.ammo > 0
                    && s.cooldown == 0
                    && d < 3.0
                    && let Some(m) = &r.stats.missile
                {
                    let list = &members[t.index()];
                    if !list.is_empty() {
                        let victim = list[rng.below(list.len() as u32) as usize] as usize;
                        let dist = positions[victim].distance(positions[i]);
                        let scatter = self.cfg.scatter_base + self.cfg.scatter_per_meter * dist;
                        let off = Vec2::new(rng.next_f32() * 2.0 - 1.0, rng.next_f32() * 2.0 - 1.0)
                            * scatter;
                        shots.push((i, positions[victim] + off));
                        s.ammo -= 1;
                        s.cooldown = (m.reload_ticks + rng.below(m.reload_ticks / 3 + 1))
                            .min(u32::from(u16::MAX)) as u16;
                        s.since_strike = 0;
                        b.facing = (positions[victim] - positions[i]).normalize_or(b.facing);
                    }
                }
            }
            b.speed = Speed(velocity.length());
            new_pos[i] = positions[i] + velocity * dt;
            sol[i] = s;
            body[i] = b;
        }

        // 5. Мягкое расталкивание (хак H-08): перекрытие цилиндров убираем
        // частично и пропорционально массе. Считаем по старым ячейкам сетки,
        // все сдвиги складываем отдельно и применяем разом — порядок не важен.
        let mut push = vec![Vec2::ZERO; sol.len()];
        for i in 0..sol.len() {
            let ri = self.units[sol[i].unit.index()].stats.radius;
            let mi = self.units[sol[i].unit.index()].stats.mass;
            grid.for_each_near(new_pos[i], ri + self.cfg.cavalry_radius + 1.0, |j| {
                if j == i {
                    return;
                }
                let rj = self.units[sol[j].unit.index()].stats.radius;
                let mj = self.units[sol[j].unit.index()].stats.mass;
                let delta = new_pos[i] - new_pos[j];
                let min_d = ri + rj;
                let d_sq = delta.length_sq();
                if d_sq >= min_d * min_d {
                    return;
                }
                let d = d_sq.sqrt();
                // Совпавшие точки разводим в сторону, зависящую только от номеров.
                let dir = if d > 1e-4 {
                    delta * (1.0 / d)
                } else if i < j {
                    Vec2::new(1.0, 0.0)
                } else {
                    Vec2::new(-1.0, 0.0)
                };
                push[i] += dir * ((min_d - d) * self.cfg.push_stiffness * mj / (mi + mj));
            });
        }
        for i in 0..sol.len() {
            body[i].pos = new_pos[i] + push[i];
        }

        // 6. Удары.
        let mut dead = vec![false; sol.len()];
        // Удары одновременные: солдат, убитый на этом шаге, свой удар всё равно
        // наносит. Иначе выигрывала бы сторона, чьи солдаты раньше в массиве.
        for &(a, d) in &strikes {
            if dead[d] {
                continue;
            }
            let ra = &self.units[sol[a].unit.index()];
            let rd = &self.units[sol[d].unit.index()];
            let mut attack = ra.stats.melee_attack;
            if ra.charge_ticks > 0 {
                attack += ra.stats.charge_bonus;
            }
            if ra.stats.spearmen && rd.stats.cavalry {
                attack += self.cfg.spear_vs_cavalry_bonus;
            }
            let from = (body[a].pos - body[d].pos).normalize_or(body[d].facing);
            let rear = body[d].facing.dot(from) < -0.2;
            let defence = if rd.routing {
                rd.stats.armour
            } else if rear {
                rd.stats.armour + rd.stats.defence_skill / 2
            } else {
                rd.stats.armour + rd.stats.defence_skill + rd.stats.shield
            };
            let chance = (self.cfg.melee_base_chance
                + self.cfg.melee_chance_per_point * (attack - defence) as f32)
                .clamp(self.cfg.melee_min_chance, self.cfg.melee_max_chance);
            // Отдельный номер вызова: решение солдата уже потратило вызов 0.
            if roll(self.seed, RngStream::COMBAT, self.tick, ents[a], 1) < chance {
                sol[d].hp = sol[d].hp.saturating_sub(1);
                dead[d] = sol[d].hp == 0;
            }
            let (ua, ud) = (sol[a].unit.index(), sol[d].unit.index());
            self.units[ua].in_melee = true;
            self.units[ud].in_melee = true;
            if rear {
                self.units[ud].rear_hits_tick += 1;
            }
        }

        // Снаряды: долетевшие бьют, остальные летят дальше.
        let mut landed: Vec<Entity> = Vec::new();
        for (e, p) in self.world.query_mut::<(Entity, &mut Projectile)>() {
            p.age += 1;
            if p.age < p.flight {
                continue;
            }
            landed.push(e);
            let mut best: Option<(usize, f32)> = None;
            let hit_r = self.cfg.projectile_hit_radius;
            grid.for_each_near(p.to, hit_r + 1.0, |j| {
                if sol[j].side == p.side || dead[j] {
                    return;
                }
                let d = body[j].pos.distance_sq(p.to);
                let rj = self.units[sol[j].unit.index()].stats.radius;
                let lim = hit_r + rj;
                if d < lim * lim && best.is_none_or(|(_, bd)| d < bd) {
                    best = Some((j, d));
                }
            });
            if let Some((j, _)) = best {
                let rd = &self.units[sol[j].unit.index()];
                let flight_dir = (p.to - p.from).normalize_or(Vec2::NORTH);
                let frontal = flight_dir.dot(body[j].facing) < 0.0;
                let defence = rd.stats.armour + if frontal { rd.stats.shield } else { 0 };
                let chance = (self.cfg.missile_base_chance
                    + self.cfg.missile_chance_per_point * (p.attack - defence) as f32)
                    .clamp(self.cfg.missile_min_chance, self.cfg.missile_max_chance);
                if roll(self.seed, RngStream::PROJECTILES, self.tick, e, 0) < chance {
                    sol[j].hp = sol[j].hp.saturating_sub(1);
                    dead[j] = sol[j].hp == 0;
                }
            }
        }
        for e in landed {
            self.world.despawn(e);
        }
        for (i, to) in shots {
            let r = &self.units[sol[i].unit.index()];
            let Some(m) = &r.stats.missile else { continue };
            let dist = to.distance(body[i].pos);
            let flight = (dist / self.cfg.projectile_speed / dt).ceil().max(1.0) as u16;
            self.world.spawn((Projectile {
                from: body[i].pos,
                to,
                age: 0,
                flight,
                side: sol[i].side,
                attack: m.attack,
            },));
        }

        // 7. Записываем солдат обратно и убираем убитых и сбежавших.
        let edge = self.cfg.field_half_size;
        let mut removed: Vec<Entity> = Vec::new();
        for (i, (s, b)) in self
            .world
            .query_mut::<(&mut Soldier, &mut Body)>()
            .enumerate()
        {
            *s = sol[i];
            *b = body[i];
            let side = usize::from(sol[i].side);
            let u = &mut self.units[sol[i].unit.index()];
            if dead[i] {
                removed.push(ents[i]);
                u.casualties_tick += 1;
                self.killed[side] += 1;
            } else if u.routing && (b.pos.x.abs() > edge || b.pos.y.abs() > edge) {
                removed.push(ents[i]);
                self.fled[side] += 1;
            }
        }
        for e in &removed {
            if let Some(s) = self.world.get::<Soldier>(*e).copied() {
                let u = &mut self.units[s.unit.index()];
                u.alive -= 1;
                u.slots_dirty = true;
            }
            self.world.despawn(*e);
        }

        // 8. Мораль и итог.
        self.update_morale();
        if self.outcome.is_none() {
            let beaten = |side| !self.units.iter().any(|u| u.side == side && u.is_fighting());
            self.outcome = match (beaten(0), beaten(1)) {
                (true, true) => Some(Outcome::Draw),
                (true, false) => Some(Outcome::Victory { winner: 1 }),
                (false, true) => Some(Outcome::Victory { winner: 0 }),
                (false, false) => None,
            };
        }
    }

    /// Центры отрядов и раздача мест в строю после потерь.
    fn update_regiments(&mut self, sol: &mut [Soldier], body: &[Body]) {
        let mut sums = vec![(Vec2::ZERO, 0u32); self.units.len()];
        for (s, b) in sol.iter().zip(body) {
            let e = &mut sums[s.unit.index()];
            e.0 += b.pos;
            e.1 += 1;
        }
        for (u, (sum, n)) in self.units.iter_mut().zip(&sums) {
            if *n > 0 {
                u.center = *sum * (1.0 / *n as f32);
            }
        }
        // Места раздаются заново по порядку солдат: строй смыкается, дыр нет.
        let mut next = vec![0u16; self.units.len()];
        for s in sol.iter_mut() {
            let u = s.unit.index();
            if self.units[u].slots_dirty {
                s.slot = next[u];
                next[u] += 1;
            }
        }
        for u in &mut self.units {
            u.slots_dirty = false;
        }
    }

    /// Что отряд `u` делает на этом шаге.
    fn plan_unit(&mut self, u: usize, has_ammo: bool) -> Plan {
        let cfg = &self.cfg;
        let dt = cfg.tick_seconds;
        let r = &self.units[u];
        if !r.is_active() {
            return Plan::default();
        }
        if r.routing {
            self.units[u].state = UnitState::Routing;
            return Plan {
                flee: true,
                ..Plan::default()
            };
        }
        let has_ammo = has_ammo && r.stats.missile.is_some();
        let mut plan = Plan {
            seek: if r.in_melee {
                cfg.melee_seek_engaged
            } else {
                cfg.melee_seek_idle
            },
            ..Plan::default()
        };
        let mut state = UnitState::Ready;
        match r.order {
            Order::Hold => {
                // «Огонь по готовности»: стрелки сами бьют ближайшего врага в радиусе.
                if has_ammo && !r.in_melee {
                    plan.fire_target = self
                        .nearest_enemy_unit(u, r.stats.missile.as_ref().map_or(0.0, |m| m.range));
                }
            }
            Order::Move { dest, facing, run } => {
                let r = &mut self.units[u];
                r.anchor = dest;
                r.facing = facing;
                plan.run = run;
            }
            Order::Attack { target, run } => {
                let t = &self.units[target.index()];
                if !t.is_active() {
                    let r = &mut self.units[u];
                    r.order = Order::Hold;
                    r.anchor = r.center;
                } else {
                    let (t_center, t_depth) = (t.center, t.depth());
                    let r = &self.units[u];
                    let to = t_center - r.center;
                    let dist = to.length();
                    let dir = to.normalize_or(r.facing);
                    let range = r.stats.missile.as_ref().map_or(0.0, |m| m.range);
                    let r = &mut self.units[u];
                    r.facing = dir;
                    if has_ammo && !r.in_melee {
                        if dist <= range * 0.95 {
                            if r.state != UnitState::Firing {
                                r.anchor = r.center;
                            }
                            plan.fire_target = Some(target);
                        } else {
                            r.anchor = t_center - dir * (range * 0.85);
                            plan.run = run;
                        }
                    } else {
                        // Вбегаем в строй врага: точка построения — его центр,
                        // поэтому первая шеренга упирается во вражескую.
                        r.anchor = t_center - dir * (t_depth * 0.25);
                        let close = dist < cfg.engage_distance + t_depth;
                        plan.run = run || close;
                        if close || r.in_melee {
                            plan.seek = cfg.melee_seek_engaged;
                        }
                        if plan.run && !r.in_melee {
                            r.run_distance += r.stats.run_speed * dt;
                        }
                    }
                }
            }
        }
        let r = &mut self.units[u];
        // Строй не уходит за край поля: туда попадают только бегущие.
        let lim = cfg.field_half_size - 15.0;
        r.anchor = Vec2::new(r.anchor.x.clamp(-lim, lim), r.anchor.y.clamp(-lim, lim));
        // Натиск: первый контакт после разбега даёт бонус атаки на несколько секунд.
        if r.in_melee && r.run_distance >= cfg.charge_min_run {
            r.charge_ticks = cfg.ticks(cfg.charge_seconds);
            r.run_distance = 0.0;
        }
        r.charge_ticks = r.charge_ticks.saturating_sub(1);
        if r.in_melee {
            state = UnitState::Melee;
        } else if plan.fire_target.is_some() {
            state = UnitState::Firing;
        } else if r.center.distance(r.anchor + r.facing * (-0.5 * r.depth())) > 3.0 {
            state = UnitState::Moving;
        }
        r.state = state;
        // Флаг рукопашной обновится по ударам этого шага.
        r.in_melee = false;
        plan
    }

    /// Ближайший боеспособный вражеский отряд в радиусе `range`.
    pub(crate) fn nearest_enemy_unit(&self, u: usize, range: f32) -> Option<UnitId> {
        let r = &self.units[u];
        self.units
            .iter()
            .filter(|t| t.side != r.side && t.is_fighting())
            .map(|t| (t.id, t.center.distance_sq(r.center)))
            .filter(|&(_, d)| d <= range * range)
            .min_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)))
            .map(|(id, _)| id)
    }

    fn update_morale(&mut self) {
        let cfg = &self.cfg;
        let dt = cfg.tick_seconds;
        let mut new_routs: Vec<usize> = Vec::new();
        for u in 0..self.units.len() {
            let enemy_near = {
                let r = &self.units[u];
                self.units.iter().any(|t| {
                    t.side != r.side
                        && t.is_fighting()
                        && t.center.distance_sq(r.center)
                            < cfg.rally_clear_radius * cfg.rally_clear_radius
                })
            };
            let r = &mut self.units[u];
            if !r.is_active() {
                continue;
            }
            if r.alive == 0 {
                r.state = UnitState::Gone;
                r.routing = false;
                continue;
            }
            let sens = r.stats.morale_sensitivity;
            let loss = sens
                * (r.casualties_tick as f32 / r.initial_count as f32 * cfg.morale_casualty_weight
                    + r.rear_hits_tick as f32 * cfg.morale_rear_hit);
            r.morale -= loss;
            // Отряд, потерявший три четверти, тает сам по себе.
            if r.alive * 4 < r.initial_count {
                r.morale -= sens * 2.0 * dt;
            }
            let quiet = r.casualties_tick == 0 && r.rear_hits_tick == 0;
            if r.routing {
                if !enemy_near {
                    r.morale = (r.morale + cfg.morale_recovery * 2.0 * dt).min(r.stats.morale);
                }
                if !r.rallied_once && !enemy_near && r.morale >= r.stats.morale * cfg.rally_fraction
                {
                    r.routing = false;
                    r.rallied_once = true;
                    r.order = Order::Hold;
                    r.anchor = r.center;
                    r.state = UnitState::Ready;
                }
            } else {
                if quiet && !r.in_melee {
                    r.morale = (r.morale + cfg.morale_recovery * dt).min(r.stats.morale);
                }
                if r.morale <= 0.0 && sens > 0.0 {
                    r.routing = true;
                    r.morale = -10.0;
                    r.state = UnitState::Routing;
                    new_routs.push(u);
                }
            }
            r.casualties_tick = 0;
            r.rear_hits_tick = 0;
        }
        // Бегство соседа пугает союзников рядом.
        for u in new_routs {
            let (side, center) = (self.units[u].side, self.units[u].center);
            for t in &mut self.units {
                if t.side == side
                    && t.is_fighting()
                    && t.center.distance_sq(center) < cfg.ally_rout_radius * cfg.ally_rout_radius
                {
                    t.morale -= cfg.morale_ally_rout * t.stats.morale_sensitivity;
                }
            }
        }
    }
}

/// Ближайший к солдату `i` вражеский солдат в радиусе `radius`.
fn nearest_enemy(
    grid: &Grid,
    positions: &[Vec2],
    sol: &[Soldier],
    i: usize,
    radius: f32,
) -> Option<usize> {
    let p = positions[i];
    let side = sol[i].side;
    let mut best: Option<(usize, f32)> = None;
    grid.for_each_near(p, radius, |j| {
        if sol[j].side == side {
            return;
        }
        let d = positions[j].distance_sq(p);
        if d <= radius * radius && best.is_none_or(|(bj, bd)| d < bd || (d == bd && j < bj)) {
            best = Some((j, d));
        }
    });
    best.map(|(j, _)| j)
}

/// Бросок кубика `[0, 1)` как чистая функция ключа (см. `rm_core::sim::random_u64`).
fn roll(seed: u64, stream: RngStream, tick: u64, entity: Entity, call: u64) -> f32 {
    (random_u64(seed, stream, tick, entity.to_bits(), call) >> 40) as f32
        * (1.0 / (1u64 << 24) as f32)
}
