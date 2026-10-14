//! Entity Component System на архетипах.
//!
//! Ключевые понятия:
//!
//! * [`Entity`] — просто номер (индекс + поколение). Никаких данных внутри.
//! * Компонент — любая `'static + Send + Sync` структура (позиция, здоровье,
//!   отряд…). Отдельно ничего реализовывать не нужно.
//! * Архетип — набор сущностей с *одинаковым* набором компонентов. Для каждого
//!   типа компонента у архетипа своя плотная колонка в памяти.
//! * [`World`] — владеет всеми архетипами и выдаёт запросы (`query`).
//!
//! Пример:
//! ```
//! use rm_core::ecs::{World, Entity};
//!
//! #[derive(Debug, PartialEq)]
//! struct Pos(f32);
//! struct Vel(f32);
//!
//! let mut world = World::new();
//! let e: Entity = world.spawn((Pos(0.0), Vel(2.0)));
//! for (pos, vel) in world.query_mut::<(&mut Pos, &Vel)>() {
//!     pos.0 += vel.0;
//! }
//! assert_eq!(world.get::<Pos>(e), Some(&Pos(2.0)));
//! ```

mod archetype;
mod blob;
mod bundle;
mod component;
mod entity;
mod query;
mod world;

pub use archetype::{Archetype, ArchetypeId};
pub use bundle::Bundle;
pub use component::{Component, ComponentId, ComponentInfo, ComponentRegistry};
pub use entity::Entity;
pub use query::{QueryIter, ReadOnlyQuery, WorldQuery};
pub use world::World;
