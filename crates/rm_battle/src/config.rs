//! Настройки боя, которых нет в `export_descr_unit.txt`.
//!
//! Статы отрядов (атака, броня, мораль, число солдат) берутся из данных.
//! Здесь — общие правила боя: скорости шага и бега, дальность удара,
//! формулы шанса попадания, мораль. В оригинале такие числа лежат
//! в отдельных файлах (`descr_mount.txt`, `battle_config.xml`).
//! Пока они собраны в одном месте, чтобы их было легко перенести в
//! `data/` вместе с парсером в `rm_data`; см. `docs/battle/README.md`.

/// Общие правила боя.
#[derive(Debug, Clone, PartialEq)]
pub struct BattleConfig {
    /// Длительность одного шага симуляции, секунды (20 шагов в секунду).
    pub tick_seconds: f32,
    /// Половина стороны квадратного поля, метры. Отряд, убежавший за край, покидает битву.
    pub field_half_size: f32,

    /// Скорость пехоты шагом и бегом, м/с.
    pub infantry_walk: f32,
    pub infantry_run: f32,
    /// Скорость конницы шагом и бегом, м/с.
    pub cavalry_walk: f32,
    pub cavalry_run: f32,
    /// Радиус цилиндра пехотинца и всадника, метры (хак H-06).
    pub infantry_radius: f32,
    pub cavalry_radius: f32,
    /// Жёсткость расталкивания: какую долю перекрытия цилиндров убираем за шаг (хак H-08).
    pub push_stiffness: f32,

    /// Дистанция удара между краями цилиндров, метры.
    pub melee_reach: f32,
    /// Радиус, в котором солдат стоящего отряда сам замечает врага и вступает в бой.
    pub melee_seek_idle: f32,
    /// Радиус поиска врага, когда отряд уже в рукопашной.
    pub melee_seek_engaged: f32,
    /// Расстояние между центрами отрядов, на котором атакующий отряд ломает строй и бросается в бой.
    pub engage_distance: f32,
    /// Шанс убить ударом: `base + per_point × (атака − защита)`, в пределах `[min, max]`.
    pub melee_base_chance: f32,
    pub melee_chance_per_point: f32,
    pub melee_min_chance: f32,
    pub melee_max_chance: f32,
    /// Бонус копейщиков к атаке против конницы.
    pub spear_vs_cavalry_bonus: i32,
    /// Сколько секунд после первого контакта действует бонус натиска.
    pub charge_seconds: f32,
    /// Минимальный разгон перед натиском: отряд должен пробежать хотя бы столько метров.
    pub charge_min_run: f32,

    /// Скорость полёта снаряда, м/с.
    pub projectile_speed: f32,
    /// Разброс точки падения: `base + per_meter × дистанция`, метры.
    pub scatter_base: f32,
    pub scatter_per_meter: f32,
    /// Радиус попадания снаряда вокруг центра солдата, метры.
    pub projectile_hit_radius: f32,
    /// Шанс убить попавшим снарядом: `base + per_point × (атака − броня − щит)`.
    pub missile_base_chance: f32,
    pub missile_chance_per_point: f32,
    pub missile_min_chance: f32,
    pub missile_max_chance: f32,
    /// Перезарядка арбалета, лука и прочего метательного оружия, секунды.
    pub reload_crossbow: f32,
    pub reload_bow: f32,
    pub reload_other: f32,

    /// Начальная мораль: `base + per_point × stat_mental`.
    pub morale_base: f32,
    pub morale_per_point: f32,
    /// Потеря морали за потерю всего отряда (пропорционально доле убитых за шаг).
    pub morale_casualty_weight: f32,
    /// Потеря морали за каждый удар в спину или фланг.
    pub morale_rear_hit: f32,
    /// Разовая потеря, когда рядом (в пределах `ally_rout_radius`) бежит союзный отряд.
    pub morale_ally_rout: f32,
    pub ally_rout_radius: f32,
    /// Восстановление морали вне боя, очков в секунду.
    pub morale_recovery: f32,
    /// Бегущий отряд может один раз собраться, если мораль восстановилась до этой доли
    /// от начальной, а врагов ближе `rally_clear_radius` нет.
    pub rally_fraction: f32,
    pub rally_clear_radius: f32,
}

impl Default for BattleConfig {
    fn default() -> Self {
        Self {
            tick_seconds: 0.05,
            field_half_size: 350.0,
            infantry_walk: 2.2,
            infantry_run: 4.8,
            cavalry_walk: 3.8,
            cavalry_run: 9.0,
            infantry_radius: 0.4,
            cavalry_radius: 0.9,
            push_stiffness: 0.5,
            melee_reach: 0.7,
            melee_seek_idle: 2.5,
            melee_seek_engaged: 18.0,
            engage_distance: 28.0,
            melee_base_chance: 0.18,
            melee_chance_per_point: 0.03,
            melee_min_chance: 0.03,
            melee_max_chance: 0.6,
            spear_vs_cavalry_bonus: 4,
            charge_seconds: 3.0,
            charge_min_run: 15.0,
            projectile_speed: 55.0,
            scatter_base: 0.8,
            scatter_per_meter: 0.03,
            projectile_hit_radius: 0.6,
            missile_base_chance: 0.2,
            missile_chance_per_point: 0.05,
            missile_min_chance: 0.05,
            missile_max_chance: 0.85,
            reload_crossbow: 8.0,
            reload_bow: 3.5,
            reload_other: 4.0,
            morale_base: 40.0,
            morale_per_point: 8.0,
            morale_casualty_weight: 120.0,
            morale_rear_hit: 0.35,
            morale_ally_rout: 12.0,
            ally_rout_radius: 40.0,
            morale_recovery: 1.5,
            rally_fraction: 0.45,
            rally_clear_radius: 45.0,
        }
    }
}

impl BattleConfig {
    /// Переводит секунды в целое число шагов (не меньше одного).
    pub fn ticks(&self, seconds: f32) -> u32 {
        ((seconds / self.tick_seconds) + 0.5).max(1.0) as u32
    }
}
