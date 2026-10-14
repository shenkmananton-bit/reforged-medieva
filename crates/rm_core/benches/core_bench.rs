//! Бенчмарки ядра. Запуск: `cargo bench -p rm_core`.
//!
//! Собственный маленький замерщик вместо criterion — чтобы не тянуть
//! зависимости в ядро. Каждый замер повторяется несколько раз, печатается
//! медиана и минимум, а также время на одну сущность.

use std::hint::black_box;
use std::time::{Duration, Instant};

use rm_core::sim::{Rng, RngStream};
use rm_core::{Arena, Entity, JobSystem, World};

#[derive(Clone, Copy)]
struct Pos {
    x: f32,
    y: f32,
}
#[derive(Clone, Copy)]
struct Vel {
    x: f32,
    y: f32,
}
#[derive(Clone, Copy)]
struct Hp(u16);
#[derive(Clone, Copy)]
struct Morale(i16);
struct Cavalry;
struct Archer;

/// «ООП-подход» для сравнения: каждый солдат — отдельный объект в куче.
#[allow(dead_code)]
struct SoldierObject {
    pos: Pos,
    vel: Vel,
    hp: Hp,
    morale: Morale,
    name: String,
    padding: [u64; 8],
}

fn bench<F: FnMut()>(name: &str, items: usize, iters: u32, mut f: F) {
    f(); // прогрев
    let mut times: Vec<Duration> = (0..iters)
        .map(|_| {
            let t = Instant::now();
            f();
            t.elapsed()
        })
        .collect();
    times.sort();
    let median = times[times.len() / 2];
    let min = times[0];
    println!(
        "{name:<52} {:>10.1} мкс  (мин {:>9.1} мкс)  {:>7.2} нс/шт",
        median.as_secs_f64() * 1e6,
        min.as_secs_f64() * 1e6,
        median.as_secs_f64() * 1e9 / items as f64
    );
}

fn army(n: usize) -> World {
    let mut world = World::new();
    world.spawn_batch((0..n).map(|i| {
        let f = i as f32;
        (
            Pos { x: f, y: -f },
            Vel { x: 1.0, y: 0.5 },
            Hp(100),
            Morale(50),
        )
    }));
    world
}

/// Четыре вида отрядов → четыре архетипа (как в реальной битве).
fn mixed_army(n: usize) -> World {
    let mut world = World::new();
    for i in 0..n {
        let p = Pos {
            x: i as f32,
            y: 0.0,
        };
        let v = Vel { x: 1.0, y: 1.0 };
        match i % 4 {
            0 => world.spawn((p, v, Hp(100))),
            1 => world.spawn((p, v, Hp(100), Cavalry)),
            2 => world.spawn((p, v, Hp(100), Archer)),
            _ => world.spawn((p, v, Hp(100), Morale(10))),
        };
    }
    world
}

fn main() {
    let jobs = JobSystem::new();
    println!("rm_core бенчмарки, потоков в пуле: {}\n", jobs.threads());

    for &n in &[10_000usize, 100_000] {
        println!("--- {n} сущностей ---");

        bench(
            &format!("spawn_batch (Pos, Vel, Hp, Morale) x{n}"),
            n,
            10,
            || {
                black_box(army(n));
            },
        );

        let mut world = army(n);
        bench(&format!("query_mut: pos += vel x{n}"), n, 200, || {
            world.query_mut::<(&mut Pos, &Vel)>().for_each(|(p, v)| {
                p.x += v.x;
                p.y += v.y;
            });
        });

        bench(
            &format!("query: сумма hp (только чтение) x{n}"),
            n,
            200,
            || {
                let total: u64 = world.query::<&Hp>().map(|h| u64::from(h.0)).sum();
                black_box(total);
            },
        );

        bench(
            &format!("par_for_each_mut: pos += vel x{n}"),
            n,
            200,
            || {
                world.par_for_each_mut::<(&mut Pos, &Vel)>(&jobs, 2048, |(p, v)| {
                    p.x += v.x;
                    p.y += v.y;
                });
            },
        );

        // Тяжёлая работа на сущность — тут видна польза потоков.
        bench(
            &format!("par_for_each_mut: тяжёлая система x{n}"),
            n,
            50,
            || {
                world.par_for_each_mut::<(Entity, &mut Pos, &mut Morale)>(
                    &jobs,
                    1024,
                    |(e, p, m)| {
                        let mut rng = Rng::for_entity(7, RngStream::MORALE, 1, e.to_bits());
                        for _ in 0..32 {
                            p.x = (p.x * 1.0001 + rng.next_f32()).rem_euclid(1000.0);
                        }
                        m.0 = m.0.wrapping_add(rng.below(3) as i16 - 1);
                    },
                );
            },
        );
        bench(
            &format!("query_mut (1 поток): тяжёлая система x{n}"),
            n,
            50,
            || {
                world
                    .query_mut::<(Entity, &mut Pos, &mut Morale)>()
                    .for_each(|(e, p, m)| {
                        let mut rng = Rng::for_entity(7, RngStream::MORALE, 1, e.to_bits());
                        for _ in 0..32 {
                            p.x = (p.x * 1.0001 + rng.next_f32()).rem_euclid(1000.0);
                        }
                        m.0 = m.0.wrapping_add(rng.below(3) as i16 - 1);
                    });
            },
        );

        let mut mixed = mixed_army(n);
        bench(
            &format!("query_mut по 4 архетипам: pos += vel x{n}"),
            n,
            200,
            || {
                mixed.query_mut::<(&mut Pos, &Vel)>().for_each(|(p, v)| {
                    p.x += v.x;
                    p.y += v.y;
                });
            },
        );

        let mut objects: Vec<Box<SoldierObject>> = (0..n)
            .map(|i| {
                Box::new(SoldierObject {
                    pos: Pos {
                        x: i as f32,
                        y: 0.0,
                    },
                    vel: Vel { x: 1.0, y: 0.5 },
                    hp: Hp(100),
                    morale: Morale(50),
                    name: format!("soldier {i}"),
                    padding: [0; 8],
                })
            })
            .collect();
        // Перемешиваем порядок объектов, как бывает после долгой игры.
        Rng::new(1).shuffle(&mut objects);
        bench(
            &format!("для сравнения: Vec<Box<объект>> pos += vel x{n}"),
            n,
            200,
            || {
                for s in objects.iter_mut() {
                    s.pos.x += s.vel.x;
                    s.pos.y += s.vel.y;
                }
            },
        );

        let entities: Vec<Entity> = world.query::<Entity>().collect();
        bench(
            &format!("insert+remove компонента (смена архетипа) x{n}"),
            n,
            10,
            || {
                for &e in &entities {
                    world.insert(e, Cavalry);
                }
                for &e in &entities {
                    world.remove::<Cavalry>(e);
                }
            },
        );

        bench(&format!("despawn + spawn x{n}"), n, 10, || {
            let mut w = army(n);
            let es: Vec<Entity> = w.query::<Entity>().collect();
            for e in es {
                w.despawn(e);
            }
            black_box(w);
        });

        let mut arena = Arena::with_capacity(n * 16);
        bench(&format!("Arena: alloc Pos x{n} + reset"), n, 200, || {
            for i in 0..n {
                black_box(arena.alloc(Pos {
                    x: i as f32,
                    y: 0.0,
                }));
            }
            arena.reset();
        });
        bench(
            &format!("для сравнения: Box::new(Pos) x{n}"),
            n,
            200,
            || {
                for i in 0..n {
                    black_box(Box::new(Pos {
                        x: i as f32,
                        y: 0.0,
                    }));
                }
            },
        );

        bench(
            &format!("Rng::for_entity + 4 броска x{n}"),
            n,
            200,
            || {
                let mut acc = 0u64;
                for i in 0..n as u64 {
                    let mut r = Rng::for_entity(1, RngStream::COMBAT, 10, i);
                    acc ^= r.next_u64() ^ r.next_u64() ^ r.next_u64() ^ r.next_u64();
                }
                black_box(acc);
            },
        );
        println!();
    }
}
