# Ядро движка: `rm_core`

`rm_core` — самый нижний слой игры. На нём будут стоять битвы, кампания, ИИ и города. Здесь нет ни графики, ни правил игры: только «механика» хранения данных, многопоточности и детерминизма.

Почему решили писать своё, а не брать готовое, объясняет [docs/adr/ecs.md](../adr/ecs.md).

## Из чего состоит

| Часть | Что делает простыми словами | Код | Описание |
|---|---|---|---|
| ECS | Хранит всех солдат, отряды и армии как плотные таблицы, чтобы обходить десятки тысяч за микросекунды | `src/ecs/` | [ecs.md](ecs.md) |
| Система задач | Раздаёт работу по всем ядрам процессора | `src/jobs.rs` | [jobs.md](jobs.md) |
| Аллокаторы кадра | Очень быстрая временная память, которая очищается одним движением каждый кадр | `src/memory.rs` | [memory.md](memory.md) |
| Детерминизм | ГСЧ с seed, фиксированный шаг 20 Гц, своя тригонометрия, хеш состояния для реплеев | `src/sim/`, `src/dmath.rs` | [determinism.md](determinism.md) |
| Бенчмарки | Замеры скорости на 10 000 и 100 000 сущностей | `benches/core_bench.rs` | [benchmarks.md](benchmarks.md) |

## Как этим пользоваться: минимальный пример

```rust
use rm_core::{World, JobSystem, FixedTimestep, Entity};
use rm_core::sim::{Rng, RngStream};

struct Pos { x: f32, y: f32 }
struct Vel { x: f32, y: f32 }
struct Morale(i16);

let jobs = JobSystem::new();              // пул потоков на всю игру
let mut world = World::new();             // мир битвы
world.spawn_batch((0..10_000).map(|i| (
    Pos { x: i as f32, y: 0.0 },
    Vel { x: 1.0, y: 0.0 },
    Morale(50),
)));

let mut time = FixedTimestep::battle();   // 20 шагов в секунду
let seed = 42;
// В главном цикле, каждый кадр:
let steps = time.advance(std::time::Duration::from_millis(16));
for _ in 0..steps {
    let dt = time.step_seconds();
    let tick = time.tick();
    // Система движения: параллельно на всех ядрах.
    world.par_for_each_mut::<(&mut Pos, &Vel)>(&jobs, 1024, |(p, v)| {
        p.x += v.x * dt;
        p.y += v.y * dt;
    });
    // Система морали: случайность зависит от сущности и шага, а не от потока.
    world.par_for_each_mut::<(Entity, &mut Morale)>(&jobs, 1024, |(e, m)| {
        let mut rng = Rng::for_entity(seed, RngStream::MORALE, tick, e.to_bits());
        if rng.chance(5, 100) { m.0 -= 1; }
    });
}
```

## Проверка

```sh
cargo test  -p rm_core     # все тесты ядра
cargo bench -p rm_core     # замеры скорости (собирается в release)
```
