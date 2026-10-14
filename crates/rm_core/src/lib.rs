//! # rm_core — data-oriented ядро Reforged Medieval
//!
//! Нижний слой движка, на котором стоят кампания и битвы:
//!
//! * [`ecs`] — собственный ECS на архетипах: компоненты одного типа лежат
//!   подряд в памяти (Structure of Arrays), поэтому обход 10 000+ солдат —
//!   это линейный проход по массивам без промахов кэша.
//! * [`jobs`] — пул рабочих потоков и `parallel_for` для распараллеливания
//!   систем (обход сущностей кусками на всех ядрах).
//! * [`memory`] — линейные (bump) аллокаторы для временных данных кадра.
//! * [`sim`] — детерминированные примитивы: счётный ГСЧ, фиксированный шаг
//!   (20 Гц), стабильный хеш состояния для реплеев.
//! * [`dmath`] — `sin`/`cos`/`atan2`, одинаковые до бита на всех платформах.
//!
//! Подробное описание на русском — в `docs/core/`.

pub mod dmath;
pub mod ecs;
pub mod jobs;
pub mod memory;
pub mod sim;

pub use ecs::{Bundle, Component, Entity, World};
pub use jobs::JobSystem;
pub use memory::{Arena, FrameArenas};
pub use sim::{FixedTimestep, Rng};
