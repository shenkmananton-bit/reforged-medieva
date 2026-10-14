//! Сущности: выдача, переиспользование номеров и защита от «висячих» ссылок.

use std::fmt;

use super::archetype::ArchetypeId;

/// Дескриптор сущности: индекс слота + поколение.
///
/// Когда сущность удаляют, её слот переиспользуется, но поколение
/// увеличивается. Поэтому старый `Entity` (например, сохранённая цель атаки
/// уже убитого отряда) просто перестаёт находиться, а не указывает на чужой отряд.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Entity {
    index: u32,
    generation: u32,
}

impl Entity {
    pub fn index(self) -> u32 {
        self.index
    }

    pub fn generation(self) -> u32 {
        self.generation
    }

    /// Упаковка в `u64` (для сохранений и сети).
    pub fn to_bits(self) -> u64 {
        (u64::from(self.generation) << 32) | u64::from(self.index)
    }

    pub fn from_bits(bits: u64) -> Self {
        Self {
            index: bits as u32,
            generation: (bits >> 32) as u32,
        }
    }
}

impl fmt::Debug for Entity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Entity({}v{})", self.index, self.generation)
    }
}

/// Где лежат данные сущности: в каком архетипе и в какой строке.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct EntityLocation {
    pub archetype: ArchetypeId,
    pub row: u32,
}

struct Slot {
    generation: u32,
    /// `None` — слот свободен.
    location: Option<EntityLocation>,
}

#[derive(Default)]
pub(crate) struct Entities {
    slots: Vec<Slot>,
    free: Vec<u32>,
    alive: usize,
}

impl Entities {
    /// Выдаёт новый дескриптор. Вызывающий обязан сразу вызвать [`Self::set_location`].
    pub fn alloc(&mut self) -> Entity {
        self.alive += 1;
        if let Some(index) = self.free.pop() {
            Entity {
                index,
                generation: self.slots[index as usize].generation,
            }
        } else {
            let index = u32::try_from(self.slots.len()).expect("превышен лимит сущностей (u32)");
            self.slots.push(Slot {
                generation: 0,
                location: None,
            });
            Entity {
                index,
                generation: 0,
            }
        }
    }

    /// Освобождает слот и возвращает, где лежали данные. `None`, если
    /// сущность уже мертва.
    pub fn free(&mut self, entity: Entity) -> Option<EntityLocation> {
        let slot = self.slots.get_mut(entity.index as usize)?;
        if slot.generation != entity.generation {
            return None;
        }
        let location = slot.location.take()?;
        slot.generation = slot.generation.wrapping_add(1);
        self.free.push(entity.index);
        self.alive -= 1;
        Some(location)
    }

    #[inline]
    pub fn location(&self, entity: Entity) -> Option<EntityLocation> {
        let slot = self.slots.get(entity.index as usize)?;
        if slot.generation == entity.generation {
            slot.location
        } else {
            None
        }
    }

    #[inline]
    pub fn set_location(&mut self, entity: Entity, location: EntityLocation) {
        self.slots[entity.index as usize].location = Some(location);
    }

    pub fn len(&self) -> usize {
        self.alive
    }
}
