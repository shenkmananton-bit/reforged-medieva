//! Компоненты и реестр их типов.

use std::alloc::Layout;
use std::any::{TypeId, type_name};
use std::collections::HashMap;

/// Компонент — любые данные, которые можно повесить на сущность.
///
/// Реализован автоматически для всех `'static + Send + Sync` типов:
/// `struct Health(u16);` уже компонент. `Send + Sync` нужны, чтобы системы
/// могли обходить компоненты в нескольких потоках.
pub trait Component: Send + Sync + 'static {}
impl<T: Send + Sync + 'static> Component for T {}

/// Короткий числовой номер типа компонента внутри одного [`World`](super::World).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ComponentId(pub(crate) u32);

impl ComponentId {
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// Всё, что ECS нужно знать о типе, чтобы хранить его «без типа»:
/// размер/выравнивание и как вызвать деструктор.
#[derive(Clone, Debug)]
pub struct ComponentInfo {
    name: &'static str,
    layout: Layout,
    drop_fn: Option<unsafe fn(*mut u8)>,
}

impl ComponentInfo {
    fn of<T: Component>() -> Self {
        Self {
            name: type_name::<T>(),
            layout: Layout::new::<T>(),
            drop_fn: std::mem::needs_drop::<T>().then_some(drop_ptr::<T> as unsafe fn(*mut u8)),
        }
    }

    pub fn name(&self) -> &'static str {
        self.name
    }

    pub fn layout(&self) -> Layout {
        self.layout
    }

    pub(crate) fn drop_fn(&self) -> Option<unsafe fn(*mut u8)> {
        self.drop_fn
    }
}

unsafe fn drop_ptr<T>(ptr: *mut u8) {
    unsafe { ptr.cast::<T>().drop_in_place() }
}

/// Сопоставляет Rust-типы с [`ComponentId`].
#[derive(Default)]
pub struct ComponentRegistry {
    by_type: HashMap<TypeId, ComponentId>,
    infos: Vec<ComponentInfo>,
}

impl ComponentRegistry {
    /// Возвращает id типа, регистрируя его при первом обращении.
    pub fn register<T: Component>(&mut self) -> ComponentId {
        let infos = &mut self.infos;
        *self.by_type.entry(TypeId::of::<T>()).or_insert_with(|| {
            let id =
                ComponentId(u32::try_from(infos.len()).expect("слишком много типов компонентов"));
            infos.push(ComponentInfo::of::<T>());
            id
        })
    }

    /// id уже зарегистрированного типа или `None`.
    pub fn id<T: Component>(&self) -> Option<ComponentId> {
        self.by_type.get(&TypeId::of::<T>()).copied()
    }

    pub fn info(&self, id: ComponentId) -> &ComponentInfo {
        &self.infos[id.index()]
    }

    pub fn len(&self) -> usize {
        self.infos.len()
    }

    pub fn is_empty(&self) -> bool {
        self.infos.is_empty()
    }
}
