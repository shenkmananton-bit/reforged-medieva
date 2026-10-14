//! Запросы: «дай мне все сущности, у которых есть `Pos` и `Vel`».
//!
//! Запрос описывается типом: `(&mut Pos, &Vel)`, `(Entity, &Hp, Option<&Morale>)`.
//! Обход идёт по архетипам: для каждого подходящего архетипа берём указатели
//! на начало нужных колонок и линейно идём по строкам.

use std::marker::PhantomData;

use super::archetype::Archetype;
use super::component::{Component, ComponentId, ComponentRegistry};
use super::entity::Entity;

/// Описание одного параметра запроса (или кортежа параметров).
///
/// # Safety
/// * `access` честно перечисляет все компоненты, к которым обращается `item`,
///   с правильным флагом изменяемости.
/// * `item` должен быть безопасно вызываем из любого потока (компоненты
///   `Send + Sync`, так что для ссылок на компоненты это выполняется).
pub unsafe trait WorldQuery {
    /// Что получает пользователь на каждой строке.
    type Item<'w>;
    /// Заранее найденные id компонентов.
    type State: Copy;
    /// Указатели на начало колонок в текущем архетипе.
    type Fetch: Copy;

    /// `None` — запрос заведомо пуст (тип компонента ещё ни разу не встречался).
    fn init_state(registry: &ComponentRegistry) -> Option<Self::State>;

    /// Дописывает `(компонент, изменяемый?)` для проверки конфликтов доступа.
    fn access(state: &Self::State, out: &mut Vec<(ComponentId, bool)>);

    /// Подходит ли архетип под запрос.
    fn matches(state: &Self::State, archetype: &Archetype) -> bool;

    /// # Safety
    /// `matches(state, archetype)` вернул `true`.
    unsafe fn fetch(state: &Self::State, archetype: &Archetype) -> Self::Fetch;

    /// # Safety
    /// `row` меньше длины архетипа; правила заимствования соблюдены вызывающим
    /// (каждая изменяемая строка выдаётся не более одного раза).
    unsafe fn item<'w>(fetch: Self::Fetch, row: usize) -> Self::Item<'w>;
}

/// Запрос, который только читает. Такой можно выполнять через `&World`.
///
/// # Safety
/// Реализация не выдаёт изменяемых ссылок.
pub unsafe trait ReadOnlyQuery: WorldQuery {}

unsafe impl WorldQuery for Entity {
    type Item<'w> = Entity;
    type State = ();
    type Fetch = *const Entity;

    fn init_state(_: &ComponentRegistry) -> Option<()> {
        Some(())
    }
    fn access(_: &(), _: &mut Vec<(ComponentId, bool)>) {}
    fn matches(_: &(), _: &Archetype) -> bool {
        true
    }
    unsafe fn fetch(_: &(), archetype: &Archetype) -> *const Entity {
        archetype.entities.as_ptr()
    }
    #[inline(always)]
    unsafe fn item<'w>(fetch: *const Entity, row: usize) -> Self::Item<'w> {
        unsafe { *fetch.add(row) }
    }
}
unsafe impl ReadOnlyQuery for Entity {}

unsafe impl<T: Component> WorldQuery for &T {
    type Item<'w> = &'w T;
    type State = ComponentId;
    type Fetch = *const T;

    fn init_state(registry: &ComponentRegistry) -> Option<ComponentId> {
        registry.id::<T>()
    }
    fn access(state: &ComponentId, out: &mut Vec<(ComponentId, bool)>) {
        out.push((*state, false));
    }
    fn matches(state: &ComponentId, archetype: &Archetype) -> bool {
        archetype.has(*state)
    }
    unsafe fn fetch(state: &ComponentId, archetype: &Archetype) -> *const T {
        let column = archetype
            .column_index(*state)
            .expect("архетип не подходит под запрос");
        archetype.columns[column].as_ptr().cast()
    }
    #[inline(always)]
    unsafe fn item<'w>(fetch: *const T, row: usize) -> &'w T {
        unsafe { &*fetch.add(row) }
    }
}
unsafe impl<T: Component> ReadOnlyQuery for &T {}

unsafe impl<T: Component> WorldQuery for &mut T {
    type Item<'w> = &'w mut T;
    type State = ComponentId;
    type Fetch = *mut T;

    fn init_state(registry: &ComponentRegistry) -> Option<ComponentId> {
        registry.id::<T>()
    }
    fn access(state: &ComponentId, out: &mut Vec<(ComponentId, bool)>) {
        out.push((*state, true));
    }
    fn matches(state: &ComponentId, archetype: &Archetype) -> bool {
        archetype.has(*state)
    }
    unsafe fn fetch(state: &ComponentId, archetype: &Archetype) -> *mut T {
        let column = archetype
            .column_index(*state)
            .expect("архетип не подходит под запрос");
        archetype.columns[column].as_ptr().cast()
    }
    #[inline(always)]
    unsafe fn item<'w>(fetch: *mut T, row: usize) -> &'w mut T {
        unsafe { &mut *fetch.add(row) }
    }
}

unsafe impl<T: Component> WorldQuery for Option<&T> {
    type Item<'w> = Option<&'w T>;
    type State = Option<ComponentId>;
    type Fetch = Option<*const T>;

    fn init_state(registry: &ComponentRegistry) -> Option<Option<ComponentId>> {
        Some(registry.id::<T>())
    }
    fn access(state: &Option<ComponentId>, out: &mut Vec<(ComponentId, bool)>) {
        out.extend(state.map(|id| (id, false)));
    }
    fn matches(_: &Option<ComponentId>, _: &Archetype) -> bool {
        true
    }
    unsafe fn fetch(state: &Option<ComponentId>, archetype: &Archetype) -> Option<*const T> {
        let column = archetype.column_index((*state)?)?;
        Some(archetype.columns[column].as_ptr().cast())
    }
    #[inline(always)]
    unsafe fn item<'w>(fetch: Option<*const T>, row: usize) -> Option<&'w T> {
        fetch.map(|ptr| unsafe { &*ptr.add(row) })
    }
}
unsafe impl<T: Component> ReadOnlyQuery for Option<&T> {}

unsafe impl<T: Component> WorldQuery for Option<&mut T> {
    type Item<'w> = Option<&'w mut T>;
    type State = Option<ComponentId>;
    type Fetch = Option<*mut T>;

    fn init_state(registry: &ComponentRegistry) -> Option<Option<ComponentId>> {
        Some(registry.id::<T>())
    }
    fn access(state: &Option<ComponentId>, out: &mut Vec<(ComponentId, bool)>) {
        out.extend(state.map(|id| (id, true)));
    }
    fn matches(_: &Option<ComponentId>, _: &Archetype) -> bool {
        true
    }
    unsafe fn fetch(state: &Option<ComponentId>, archetype: &Archetype) -> Option<*mut T> {
        let column = archetype.column_index((*state)?)?;
        Some(archetype.columns[column].as_ptr().cast())
    }
    #[inline(always)]
    unsafe fn item<'w>(fetch: Option<*mut T>, row: usize) -> Option<&'w mut T> {
        fetch.map(|ptr| unsafe { &mut *ptr.add(row) })
    }
}

macro_rules! impl_query_tuple {
    ($(($Q:ident, $i:tt)),*) => {
        unsafe impl<$($Q: WorldQuery),*> WorldQuery for ($($Q,)*) {
            type Item<'w> = ($($Q::Item<'w>,)*);
            type State = ($($Q::State,)*);
            type Fetch = ($($Q::Fetch,)*);

            fn init_state(registry: &ComponentRegistry) -> Option<Self::State> {
                Some(($($Q::init_state(registry)?,)*))
            }
            fn access(state: &Self::State, out: &mut Vec<(ComponentId, bool)>) {
                $( $Q::access(&state.$i, out); )*
            }
            fn matches(state: &Self::State, archetype: &Archetype) -> bool {
                true $(&& $Q::matches(&state.$i, archetype))*
            }
            unsafe fn fetch(state: &Self::State, archetype: &Archetype) -> Self::Fetch {
                unsafe { ($($Q::fetch(&state.$i, archetype),)*) }
            }
            #[inline(always)]
            unsafe fn item<'w>(fetch: Self::Fetch, row: usize) -> Self::Item<'w> {
                unsafe { ($($Q::item(fetch.$i, row),)*) }
            }
        }
        unsafe impl<$($Q: ReadOnlyQuery),*> ReadOnlyQuery for ($($Q,)*) {}
    };
}

impl_query_tuple!((A, 0));
impl_query_tuple!((A, 0), (B, 1));
impl_query_tuple!((A, 0), (B, 1), (C, 2));
impl_query_tuple!((A, 0), (B, 1), (C, 2), (D, 3));
impl_query_tuple!((A, 0), (B, 1), (C, 2), (D, 3), (E, 4));
impl_query_tuple!((A, 0), (B, 1), (C, 2), (D, 3), (E, 4), (F, 5));
impl_query_tuple!((A, 0), (B, 1), (C, 2), (D, 3), (E, 4), (F, 5), (G, 6));
impl_query_tuple!(
    (A, 0),
    (B, 1),
    (C, 2),
    (D, 3),
    (E, 4),
    (F, 5),
    (G, 6),
    (H, 7)
);

/// Паникует, если запрос просит один компонент дважды и хотя бы раз изменяемо
/// (например `(&mut Pos, &Pos)`) — это нарушило бы правила заимствования.
pub(crate) fn assert_no_aliasing<Q: WorldQuery>(state: &Q::State, registry: &ComponentRegistry) {
    let mut access = Vec::new();
    Q::access(state, &mut access);
    for (i, &(id, mutable)) in access.iter().enumerate() {
        for &(other, other_mutable) in &access[i + 1..] {
            if id == other && (mutable || other_mutable) {
                panic!(
                    "запрос {} обращается к {} одновременно изменяемо и ещё раз",
                    std::any::type_name::<Q>(),
                    registry.info(id).name()
                );
            }
        }
    }
}

/// Итератор по результатам запроса.
pub struct QueryIter<'w, Q: WorldQuery> {
    archetypes: &'w [Archetype],
    state: Option<Q::State>,
    next_archetype: usize,
    fetch: Option<Q::Fetch>,
    row: usize,
    len: usize,
    _marker: PhantomData<fn() -> Q>,
}

impl<'w, Q: WorldQuery> QueryIter<'w, Q> {
    /// # Safety
    /// Вызывающий гарантирует, что выдаваемые ссылки не нарушат заимствования
    /// (для изменяемых запросов мир заимствован эксклюзивно на `'w`).
    pub(crate) unsafe fn new(archetypes: &'w [Archetype], state: Option<Q::State>) -> Self {
        Self {
            archetypes,
            state,
            next_archetype: 0,
            fetch: None,
            row: 0,
            len: 0,
            _marker: PhantomData,
        }
    }

    /// Переходит к следующему непустому подходящему архетипу.
    fn advance_archetype(&mut self) -> bool {
        let Some(state) = self.state.as_ref() else {
            return false;
        };
        while let Some(archetype) = self.archetypes.get(self.next_archetype) {
            self.next_archetype += 1;
            if !archetype.is_empty() && Q::matches(state, archetype) {
                // SAFETY: matches вернул true.
                self.fetch = Some(unsafe { Q::fetch(state, archetype) });
                self.row = 0;
                self.len = archetype.len();
                return true;
            }
        }
        false
    }
}

impl<'w, Q: WorldQuery> Iterator for QueryIter<'w, Q> {
    type Item = Q::Item<'w>;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if self.row < self.len {
                let row = self.row;
                self.row += 1;
                let fetch = self.fetch?;
                // SAFETY: row < len, каждая строка выдаётся один раз.
                return Some(unsafe { Q::item(fetch, row) });
            }
            if !self.advance_archetype() {
                return None;
            }
        }
    }

    /// Плотный цикл по каждому архетипу: на него опираются `for_each`/`sum`
    /// и т.п., компилятор разворачивает и векторизует его.
    fn fold<B, F: FnMut(B, Self::Item) -> B>(mut self, init: B, mut f: F) -> B {
        let mut acc = init;
        loop {
            if let Some(fetch) = self.fetch {
                for row in self.row..self.len {
                    // SAFETY: см. next().
                    acc = f(acc, unsafe { Q::item(fetch, row) });
                }
            }
            self.row = self.len;
            if !self.advance_archetype() {
                return acc;
            }
        }
    }
}
