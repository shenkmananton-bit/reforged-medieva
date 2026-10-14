//! Архетип — таблица сущностей с одинаковым набором компонентов.
//!
//! Строка таблицы — сущность, колонка — тип компонента. Колонки хранятся
//! раздельно (SoA): все `Position` подряд, все `Velocity` подряд.

use std::collections::HashMap;

use super::blob::BlobVec;
use super::component::{ComponentId, ComponentRegistry};
use super::entity::Entity;

/// Номер архетипа внутри мира.
pub type ArchetypeId = u32;

pub struct Archetype {
    /// Отсортированный список типов компонентов (это и есть «ключ» архетипа).
    pub(crate) components: Box<[ComponentId]>,
    /// `columns[i]` хранит компоненты типа `components[i]`.
    pub(crate) columns: Box<[BlobVec]>,
    /// `entities[row]` — какая сущность лежит в строке `row`.
    pub(crate) entities: Vec<Entity>,
    /// Кэш переходов «добавили компонент X → архетип Y».
    pub(crate) add_edges: HashMap<ComponentId, ArchetypeId>,
    /// Кэш переходов «убрали компонент X → архетип Y».
    pub(crate) remove_edges: HashMap<ComponentId, ArchetypeId>,
}

impl Archetype {
    pub(crate) fn new(components: Box<[ComponentId]>, registry: &ComponentRegistry) -> Self {
        debug_assert!(components.windows(2).all(|w| w[0] < w[1]));
        let columns = components
            .iter()
            .map(|&id| {
                let info = registry.info(id);
                BlobVec::new(info.layout(), info.drop_fn())
            })
            .collect();
        Self {
            components,
            columns,
            entities: Vec::new(),
            add_edges: HashMap::new(),
            remove_edges: HashMap::new(),
        }
    }

    /// Сколько сущностей в архетипе.
    pub fn len(&self) -> usize {
        self.entities.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entities.is_empty()
    }

    /// Типы компонентов архетипа (отсортированы).
    pub fn components(&self) -> &[ComponentId] {
        &self.components
    }

    /// Сущности в порядке строк.
    pub fn entities(&self) -> &[Entity] {
        &self.entities
    }

    pub fn has(&self, id: ComponentId) -> bool {
        self.column_index(id).is_some()
    }

    /// Компонентов в архетипе единицы, поэтому двоичный поиск быстрее хэша.
    #[inline]
    pub(crate) fn column_index(&self, id: ComponentId) -> Option<usize> {
        self.components.binary_search(&id).ok()
    }

    pub(crate) fn reserve(&mut self, additional: usize) {
        self.entities.reserve(additional);
        for column in self.columns.iter_mut() {
            column.reserve(additional);
        }
    }
}
