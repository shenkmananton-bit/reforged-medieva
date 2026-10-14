//! Примитивы детерминированной симуляции (см. `docs/adr/determinism.md`).
//!
//! * [`Rng`] и [`random_u64`] — счётный ГСЧ: число зависит только от
//!   `(seed, поток, шаг, сущность, номер вызова)`, а не от порядка обхода.
//! * [`FixedTimestep`] — фиксированный шаг симуляции (20 Гц для битвы).
//! * [`StableHasher`] — хеш состояния мира, одинаковый на всех платформах,
//!   для контрольных точек реплея.

mod hash;
mod rng;
mod time;

pub use hash::StableHasher;
pub use rng::{Rng, RngStream, random_u64};
pub use time::FixedTimestep;
