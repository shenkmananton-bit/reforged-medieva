//! `World` — контейнер всех сущностей, архетипов и компонентов.

use std::any::TypeId;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};

use super::archetype::{Archetype, ArchetypeId};
use super::bundle::Bundle;
use super::component::{Component, ComponentId, ComponentRegistry};
use super::entity::{Entities, Entity, EntityLocation};
use super::query::{QueryIter, ReadOnlyQuery, WorldQuery, assert_no_aliasing};
use crate::jobs::JobSystem;
use crate::sim::StableHasher;

/// Заранее посчитанный путь для bundle-типа: в какой архетип он попадает
/// и в какие колонки ложатся его поля. Делает `spawn` одним поиском в хэше.
struct BundleInfo {
    archetype: ArchetypeId,
    /// `columns[i]` — индекс колонки для `i`-го поля кортежа.
    columns: Box<[usize]>,
}

pub struct World {
    entities: Entities,
    registry: ComponentRegistry,
    archetypes: Vec<Archetype>,
    archetype_index: HashMap<Box<[ComponentId]>, ArchetypeId>,
    bundles: HashMap<TypeId, BundleInfo>,
}

impl Default for World {
    fn default() -> Self {
        Self::new()
    }
}

impl World {
    pub fn new() -> Self {
        let mut world = Self {
            entities: Entities::default(),
            registry: ComponentRegistry::default(),
            archetypes: Vec::new(),
            archetype_index: HashMap::new(),
            bundles: HashMap::new(),
        };
        // Архетип 0 — сущности без компонентов.
        world.archetype_for(&[]);
        world
    }

    /// Число живых сущностей.
    pub fn len(&self) -> usize {
        self.entities.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn contains(&self, entity: Entity) -> bool {
        self.entities.location(entity).is_some()
    }

    pub fn archetypes(&self) -> &[Archetype] {
        &self.archetypes
    }

    pub fn registry(&self) -> &ComponentRegistry {
        &self.registry
    }

    /// Регистрирует тип компонента заранее (необязательно — это делается и само).
    pub fn register<T: Component>(&mut self) -> ComponentId {
        self.registry.register::<T>()
    }

    // ---------------------------------------------------------------- создание

    /// Создаёт сущность с набором компонентов: `world.spawn((Pos(0.0), Vel(1.0)))`.
    ///
    /// # Panics
    /// Если в кортеже два компонента одного типа.
    pub fn spawn<B: Bundle>(&mut self, bundle: B) -> Entity {
        self.reserve::<B>(1);
        let info = &self.bundles[&TypeId::of::<B>()];
        // SAFETY: место зарезервировано.
        unsafe { spawn_into(&mut self.entities, &mut self.archetypes, info, bundle) }
    }

    /// Создаёт много сущностей одного вида: поиск архетипа делается один раз,
    /// память выделяется заранее.
    pub fn spawn_batch<B: Bundle, I: IntoIterator<Item = B>>(&mut self, iter: I) -> Vec<Entity> {
        let iter = iter.into_iter();
        let mut out = Vec::with_capacity(iter.size_hint().0);
        self.reserve::<B>(iter.size_hint().0);
        let info = &self.bundles[&TypeId::of::<B>()];
        for bundle in iter {
            // Подсказка размера могла соврать — проверка дешёвая.
            self.archetypes[info.archetype as usize].reserve(1);
            // SAFETY: место зарезервировано строкой выше.
            out.push(unsafe { spawn_into(&mut self.entities, &mut self.archetypes, info, bundle) });
        }
        out
    }

    /// Резервирует место под `additional` сущностей вида `B`.
    pub fn reserve<B: Bundle>(&mut self, additional: usize) {
        let archetype = self.bundle_info::<B>().archetype;
        self.archetypes[archetype as usize].reserve(additional);
    }

    fn bundle_info<B: Bundle>(&mut self) -> &BundleInfo {
        let key = TypeId::of::<B>();
        if !self.bundles.contains_key(&key) {
            let mut ids = Vec::new();
            B::register(&mut self.registry, &mut ids);
            let mut sorted = ids.clone();
            sorted.sort_unstable();
            if let Some(dup) = sorted.windows(2).find(|w| w[0] == w[1]) {
                panic!(
                    "компонент {} встречается в bundle {} дважды",
                    self.registry.info(dup[0]).name(),
                    std::any::type_name::<B>()
                );
            }
            let archetype = self.archetype_for(&sorted);
            let arch = &self.archetypes[archetype as usize];
            let columns = ids
                .iter()
                .map(|&id| arch.column_index(id).unwrap())
                .collect();
            self.bundles.insert(key, BundleInfo { archetype, columns });
        }
        &self.bundles[&key]
    }

    fn archetype_for(&mut self, sorted: &[ComponentId]) -> ArchetypeId {
        if let Some(&id) = self.archetype_index.get(sorted) {
            return id;
        }
        let id = ArchetypeId::try_from(self.archetypes.len()).expect("слишком много архетипов");
        let components: Box<[ComponentId]> = sorted.into();
        self.archetypes
            .push(Archetype::new(components.clone(), &self.registry));
        self.archetype_index.insert(components, id);
        id
    }

    // ---------------------------------------------------------------- удаление

    /// Удаляет сущность и все её компоненты. `false`, если её уже нет.
    pub fn despawn(&mut self, entity: Entity) -> bool {
        let Some(location) = self.entities.free(entity) else {
            return false;
        };
        let archetype = &mut self.archetypes[location.archetype as usize];
        let row = location.row as usize;
        for column in archetype.columns.iter_mut() {
            // SAFETY: row < len.
            unsafe { column.swap_remove_keep(row) };
        }
        archetype.entities.swap_remove(row);
        if let Some(&moved) = archetype.entities.get(row) {
            self.entities.set_location(moved, location);
        }
        // Мир уже согласован — теперь можно запускать пользовательские Drop.
        for column in archetype.columns.iter_mut() {
            // SAFETY: значение только что удалено и никуда не перенесено.
            unsafe { column.drop_removed() };
        }
        true
    }

    // ---------------------------------------------------------- доступ к данным

    pub fn get<T: Component>(&self, entity: Entity) -> Option<&T> {
        let location = self.entities.location(entity)?;
        let archetype = &self.archetypes[location.archetype as usize];
        let column = archetype.column_index(self.registry.id::<T>()?)?;
        // SAFETY: колонка создана под T, строка валидна.
        Some(unsafe { archetype.columns[column].get::<T>(location.row as usize) })
    }

    pub fn get_mut<T: Component>(&mut self, entity: Entity) -> Option<&mut T> {
        let location = self.entities.location(entity)?;
        let archetype = &mut self.archetypes[location.archetype as usize];
        let column = archetype.column_index(self.registry.id::<T>()?)?;
        // SAFETY: колонка создана под T, строка валидна.
        Some(unsafe { archetype.columns[column].get_mut::<T>(location.row as usize) })
    }

    pub fn has<T: Component>(&self, entity: Entity) -> bool {
        self.get::<T>(entity).is_some()
    }

    // ------------------------------------------------- добавление/снятие компонента

    /// Добавляет (или заменяет) компонент. Сущность переезжает в другой
    /// архетип — это дороже, чем менять значение, поэтому для частых
    /// переключений лучше хранить флаг внутри компонента.
    /// Возвращает `false`, если сущности нет.
    pub fn insert<T: Component>(&mut self, entity: Entity, value: T) -> bool {
        let Some(location) = self.entities.location(entity) else {
            return false;
        };
        let id = self.registry.register::<T>();
        let src = location.archetype;
        if let Some(column) = self.archetypes[src as usize].column_index(id) {
            // SAFETY: колонка под T.
            unsafe {
                *self.archetypes[src as usize].columns[column]
                    .get_mut::<T>(location.row as usize) = value;
            }
            return true;
        }
        let dst = self.add_edge(src, id);
        self.archetypes[dst as usize].reserve(1);
        // SAFETY: место в dst зарезервировано, dst ⊇ src.
        unsafe { self.move_entity(entity, location, dst) };
        let archetype = &mut self.archetypes[dst as usize];
        let column = archetype.column_index(id).unwrap();
        let value = std::mem::ManuallyDrop::new(value);
        // SAFETY: место есть, значение передаётся во владение колонке.
        unsafe { archetype.columns[column].push_raw((&*value as *const T).cast()) };
        true
    }

    /// Снимает компонент и возвращает его значение.
    pub fn remove<T: Component>(&mut self, entity: Entity) -> Option<T> {
        let location = self.entities.location(entity)?;
        let id = self.registry.id::<T>()?;
        let src = location.archetype;
        let column = self.archetypes[src as usize].column_index(id)?;
        let dst = self.remove_edge(src, id);
        self.archetypes[dst as usize].reserve(1);
        // SAFETY: место есть; колонка T осталась «снятой» в src.
        unsafe { self.move_entity(entity, location, dst) };
        let ptr = self.archetypes[src as usize].columns[column].removed_ptr();
        // SAFETY: значение T снято и никуда не перенесено — забираем его.
        Some(unsafe { ptr.cast::<T>().read() })
    }

    fn add_edge(&mut self, src: ArchetypeId, id: ComponentId) -> ArchetypeId {
        if let Some(&dst) = self.archetypes[src as usize].add_edges.get(&id) {
            return dst;
        }
        let mut components = self.archetypes[src as usize].components.to_vec();
        let pos = components.binary_search(&id).unwrap_err();
        components.insert(pos, id);
        let dst = self.archetype_for(&components);
        self.archetypes[src as usize].add_edges.insert(id, dst);
        self.archetypes[dst as usize].remove_edges.insert(id, src);
        dst
    }

    fn remove_edge(&mut self, src: ArchetypeId, id: ComponentId) -> ArchetypeId {
        if let Some(&dst) = self.archetypes[src as usize].remove_edges.get(&id) {
            return dst;
        }
        let components: Vec<_> = self.archetypes[src as usize]
            .components
            .iter()
            .copied()
            .filter(|&c| c != id)
            .collect();
        let dst = self.archetype_for(&components);
        self.archetypes[src as usize].remove_edges.insert(id, dst);
        self.archetypes[dst as usize].add_edges.insert(id, src);
        dst
    }

    /// Переносит строку сущности из её архетипа в `dst`. Общие компоненты
    /// переезжают побайтно; компоненты, которых нет в `dst`, остаются
    /// «снятыми» в src (см. `BlobVec::removed_ptr`) — с ними разбирается
    /// вызывающий. Колонки `dst`, которых не было в src, вызывающий обязан
    /// дозаполнить сразу после вызова.
    ///
    /// # Safety
    /// В `dst` зарезервирована одна строка; `src != dst`.
    unsafe fn move_entity(&mut self, entity: Entity, location: EntityLocation, dst: ArchetypeId) {
        let src = location.archetype;
        let row = location.row as usize;
        let (src_arch, dst_arch) = two_mut(&mut self.archetypes, src as usize, dst as usize);
        for (i, column) in src_arch.columns.iter_mut().enumerate() {
            unsafe { column.swap_remove_keep(row) };
            if let Some(dst_column) = dst_arch.column_index(src_arch.components[i]) {
                unsafe { dst_arch.columns[dst_column].push_raw(column.removed_ptr()) };
            }
        }
        src_arch.entities.swap_remove(row);
        if let Some(&moved) = src_arch.entities.get(row) {
            self.entities.set_location(moved, location);
        }
        let new_row = dst_arch.entities.len() as u32;
        dst_arch.entities.push(entity);
        self.entities.set_location(
            entity,
            EntityLocation {
                archetype: dst,
                row: new_row,
            },
        );
    }

    // ---------------------------------------------------------------- запросы

    /// Запрос только на чтение: `world.query::<(Entity, &Pos)>()`.
    pub fn query<Q: ReadOnlyQuery>(&self) -> QueryIter<'_, Q> {
        let state = Q::init_state(&self.registry);
        // SAFETY: запрос только читает, мир заимствован на чтение.
        unsafe { QueryIter::new(&self.archetypes, state) }
    }

    /// Запрос с изменением: `world.query_mut::<(&mut Pos, &Vel)>()`.
    ///
    /// # Panics
    /// Если запрос просит один компонент и изменяемо, и ещё раз.
    pub fn query_mut<Q: WorldQuery>(&mut self) -> QueryIter<'_, Q> {
        let state = Q::init_state(&self.registry);
        if let Some(state) = &state {
            assert_no_aliasing::<Q>(state, &self.registry);
        }
        // SAFETY: мир заимствован эксклюзивно на всё время жизни итератора,
        // конфликтов внутри запроса нет.
        unsafe { QueryIter::new(&self.archetypes, state) }
    }

    /// Параллельный обход: строки режутся на куски по `batch` штук и
    /// раздаются потокам `jobs`. Каждая строка обрабатывается ровно одним
    /// потоком, поэтому `&mut` безопасен.
    ///
    /// Порядок обработки строк не определён — `f` не должна зависеть от
    /// него (для детерминизма берите случайность из `Rng::for_entity`).
    pub fn par_for_each_mut<'w, Q: WorldQuery>(
        &'w mut self,
        jobs: &JobSystem,
        batch: usize,
        f: impl Fn(Q::Item<'w>) + Sync,
    ) {
        let state = Q::init_state(&self.registry);
        if let Some(state) = &state {
            assert_no_aliasing::<Q>(state, &self.registry);
        }
        // SAFETY: мир заимствован эксклюзивно, конфликтов нет.
        unsafe { par_for_each_impl::<Q>(&self.archetypes, state, jobs, batch, f) }
    }

    /// Детерминированный хеш всех компонентов типа `T` вместе с id их
    /// сущностей — для контрольных точек реплея (`docs/adr/determinism.md`).
    /// Порядок обхода (архетипы в порядке создания, строки по порядку) зависит
    /// только от последовательности операций над миром, поэтому два прогона
    /// с одинаковыми командами дают одинаковый хеш.
    pub fn hash_component<T: Component + Hash>(&self) -> u64 {
        let mut hasher = StableHasher::new();
        for (entity, component) in self.query::<(Entity, &T)>() {
            hasher.write_u64(entity.to_bits());
            component.hash(&mut hasher);
        }
        hasher.finish()
    }

    /// Параллельный обход только на чтение.
    pub fn par_for_each<'w, Q: ReadOnlyQuery>(
        &'w self,
        jobs: &JobSystem,
        batch: usize,
        f: impl Fn(Q::Item<'w>) + Sync,
    ) {
        let state = Q::init_state(&self.registry);
        // SAFETY: только чтение.
        unsafe { par_for_each_impl::<Q>(&self.archetypes, state, jobs, batch, f) }
    }
}

/// Кладёт bundle в его архетип и выдаёт сущность.
///
/// # Safety
/// В архетипе `info.archetype` зарезервировано место хотя бы под одну строку.
unsafe fn spawn_into<B: Bundle>(
    entities: &mut Entities,
    archetypes: &mut [Archetype],
    info: &BundleInfo,
    bundle: B,
) -> Entity {
    let entity = entities.alloc();
    let archetype = &mut archetypes[info.archetype as usize];
    let row = archetype.entities.len();
    let columns = &mut archetype.columns;
    // SAFETY: каждое поле копируется в свою колонку ровно один раз, место есть.
    unsafe {
        bundle.write(&mut |field, ptr| columns[info.columns[field]].push_raw(ptr));
    }
    archetype.entities.push(entity);
    let location = EntityLocation {
        archetype: info.archetype,
        row: row as u32,
    };
    entities.set_location(entity, location);
    entity
}

/// Обёртка, разрешающая делиться сырыми указателями между потоками.
/// Безопасность обеспечивает разбиение на непересекающиеся диапазоны строк.
struct SyncCell<T>(T);
unsafe impl<T> Sync for SyncCell<T> {}
impl<T> SyncCell<T> {
    fn get(&self) -> &T {
        &self.0
    }
}

/// # Safety
/// Как у `QueryIter::new`.
unsafe fn par_for_each_impl<'w, Q: WorldQuery>(
    archetypes: &'w [Archetype],
    state: Option<Q::State>,
    jobs: &JobSystem,
    batch: usize,
    f: impl Fn(Q::Item<'w>) + Sync,
) {
    let Some(state) = state else { return };
    let batch = batch.max(1);
    let mut tasks = Vec::new();
    for archetype in archetypes {
        if archetype.is_empty() || !Q::matches(&state, archetype) {
            continue;
        }
        // SAFETY: matches вернул true.
        let fetch = unsafe { Q::fetch(&state, archetype) };
        let len = archetype.len();
        let mut start = 0;
        while start < len {
            let end = (start + batch).min(len);
            tasks.push((fetch, start, end));
            start = end;
        }
    }
    let tasks = SyncCell(tasks);
    jobs.parallel_for(tasks.get().len(), |i| {
        let (fetch, start, end) = tasks.get()[i];
        for row in start..end {
            // SAFETY: диапазоны строк не пересекаются, каждая строка — один раз.
            f(unsafe { Q::item(fetch, row) });
        }
    });
}

fn two_mut<T>(slice: &mut [T], a: usize, b: usize) -> (&mut T, &mut T) {
    assert_ne!(a, b);
    if a < b {
        let (lo, hi) = slice.split_at_mut(b);
        (&mut lo[a], &mut hi[0])
    } else {
        let (lo, hi) = slice.split_at_mut(a);
        (&mut hi[0], &mut lo[b])
    }
}

#[cfg(test)]
impl World {
    /// Проверяет внутренние инварианты: длины колонок совпадают, локации верны.
    fn assert_invariants(&self) {
        let mut alive = 0;
        for (id, archetype) in self.archetypes.iter().enumerate() {
            for column in archetype.columns.iter() {
                assert_eq!(column.len(), archetype.entities.len());
            }
            for (row, &entity) in archetype.entities.iter().enumerate() {
                let location = self
                    .entities
                    .location(entity)
                    .expect("мёртвая сущность в архетипе");
                assert_eq!(
                    location,
                    EntityLocation {
                        archetype: id as ArchetypeId,
                        row: row as u32
                    }
                );
                alive += 1;
            }
        }
        assert_eq!(alive, self.len());
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;
    use crate::sim::Rng;

    #[derive(Debug, Clone, PartialEq, Hash)]
    struct A(u32);
    #[derive(Debug, Clone, PartialEq, Hash)]
    struct B(String);
    #[derive(Debug, Clone, PartialEq, Hash)]
    struct Tag;

    type Model = BTreeMap<Entity, (Option<A>, Option<B>, Option<Tag>)>;

    /// Случайные операции над миром сверяются с простой моделью на BTreeMap.
    #[test]
    fn random_operations_match_model() {
        let mut world = World::new();
        let mut model = Model::new();
        let mut rng = Rng::new(12345);
        for step in 0..20_000u32 {
            let pick = |rng: &mut Rng, model: &BTreeMap<Entity, _>| -> Option<Entity> {
                if model.is_empty() {
                    return None;
                }
                model
                    .keys()
                    .nth(rng.below(model.len() as u32) as usize)
                    .copied()
            };
            match rng.below(7) {
                0 | 1 => {
                    let e = world.spawn((A(step), B(format!("b{step}"))));
                    model.insert(e, (Some(A(step)), Some(B(format!("b{step}"))), None));
                }
                2 => {
                    if let Some(e) = pick(&mut rng, &model) {
                        assert!(world.despawn(e));
                        assert!(!world.despawn(e));
                        model.remove(&e);
                    }
                }
                3 => {
                    if let Some(e) = pick(&mut rng, &model) {
                        world.insert(e, Tag);
                        model.get_mut(&e).unwrap().2 = Some(Tag);
                    }
                }
                4 => {
                    if let Some(e) = pick(&mut rng, &model) {
                        let got = world.remove::<B>(e);
                        assert_eq!(got, model.get_mut(&e).unwrap().1.take());
                    }
                }
                5 => {
                    if let Some(e) = pick(&mut rng, &model) {
                        world.insert(e, A(step * 2));
                        model.get_mut(&e).unwrap().0 = Some(A(step * 2));
                    }
                }
                _ => {
                    if let Some(e) = pick(&mut rng, &model) {
                        world.remove::<A>(e);
                        model.get_mut(&e).unwrap().0 = None;
                    }
                }
            }
            if step % 997 == 0 {
                world.assert_invariants();
            }
        }
        world.assert_invariants();
        assert_eq!(world.len(), model.len());
        for (e, (a, b, t)) in &model {
            assert_eq!(world.get::<A>(*e), a.as_ref());
            assert_eq!(world.get::<B>(*e), b.as_ref());
            assert_eq!(world.get::<Tag>(*e), t.as_ref());
        }
        let queried = world.query::<(Entity, &A, Option<&B>)>().count();
        assert_eq!(queried, model.values().filter(|m| m.0.is_some()).count());
    }

    struct DropCounter(Arc<AtomicUsize>);
    impl Drop for DropCounter {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::Relaxed);
        }
    }

    #[test]
    fn components_are_dropped_exactly_once() {
        let drops = Arc::new(AtomicUsize::new(0));
        {
            let mut world = World::new();
            let es: Vec<Entity> = (0..100)
                .map(|_| world.spawn((DropCounter(drops.clone()), A(0))))
                .collect();
            for e in &es[..30] {
                world.despawn(*e);
            }
            assert_eq!(drops.load(Ordering::Relaxed), 30);
            for e in &es[30..40] {
                world.insert(*e, Tag); // переезд не должен дропать
            }
            assert_eq!(drops.load(Ordering::Relaxed), 30);
            let taken = world.remove::<DropCounter>(es[50]).unwrap();
            assert_eq!(drops.load(Ordering::Relaxed), 30);
            drop(taken);
            assert_eq!(drops.load(Ordering::Relaxed), 31);
            world.insert(es[60], DropCounter(drops.clone())); // замена старого значения
            assert_eq!(drops.load(Ordering::Relaxed), 32);
        }
        // 100 созданных + 1 вставленный = 101.
        assert_eq!(drops.load(Ordering::Relaxed), 101);
    }

    #[test]
    fn stale_entity_is_rejected() {
        let mut world = World::new();
        let old = world.spawn((A(1),));
        world.despawn(old);
        let new = world.spawn((A(2),));
        assert_eq!(old.index(), new.index(), "слот переиспользован");
        assert_ne!(old, new);
        assert_eq!(world.get::<A>(old), None);
        assert!(!world.insert(old, Tag));
        assert_eq!(world.get::<A>(new), Some(&A(2)));
    }

    #[test]
    #[should_panic(expected = "дважды")]
    fn duplicate_component_in_bundle_panics() {
        World::new().spawn((A(1), A(2)));
    }

    #[test]
    #[should_panic(expected = "изменяемо")]
    fn aliasing_query_panics() {
        let mut world = World::new();
        world.spawn((A(1),));
        let _ = world.query_mut::<(&mut A, &A)>();
    }

    #[test]
    fn query_for_unknown_component_is_empty() {
        let mut world = World::new();
        world.spawn((A(1),));
        assert_eq!(world.query::<&B>().count(), 0);
        assert_eq!(world.query::<(&A, Option<&B>)>().count(), 1);
    }

    #[test]
    fn zero_sized_tags_work() {
        let mut world = World::new();
        for i in 0..1000 {
            if i % 3 == 0 {
                world.spawn((A(i), Tag));
            } else {
                world.spawn((A(i),));
            }
        }
        assert_eq!(world.query::<&Tag>().count(), 334);
        assert_eq!(
            world
                .query::<(&A, &Tag)>()
                .map(|(a, _)| a.0 % 3)
                .sum::<u32>(),
            0
        );
    }

    #[test]
    fn par_for_each_matches_serial() {
        let jobs = JobSystem::with_threads(4);
        let mut world = World::new();
        for i in 0..50_000u32 {
            if i % 2 == 0 {
                world.spawn((A(i),));
            } else {
                world.spawn((A(i), Tag));
            }
        }
        world.par_for_each_mut::<&mut A>(&jobs, 777, |a| a.0 = a.0 * 3 + 1);
        let mut expected: Vec<u32> = (0..50_000u32).map(|i| i * 3 + 1).collect();
        let mut got: Vec<u32> = world.query::<&A>().map(|a| a.0).collect();
        expected.sort_unstable();
        got.sort_unstable();
        assert_eq!(got, expected);
        let seen = AtomicUsize::new(0);
        world.par_for_each::<(&A, &Tag)>(&jobs, 100, |_| {
            seen.fetch_add(1, Ordering::Relaxed);
        });
        assert_eq!(seen.into_inner(), 25_000);
    }

    #[test]
    fn same_commands_same_hash() {
        let run = |threads: usize| {
            let jobs = JobSystem::with_threads(threads);
            let mut world = World::new();
            for i in 0..5000u32 {
                world.spawn((A(i),));
            }
            let es: Vec<Entity> = world.query::<Entity>().collect();
            for e in es.iter().step_by(7) {
                world.despawn(*e);
            }
            for tick in 0..10u64 {
                world.par_for_each_mut::<(Entity, &mut A)>(&jobs, 256, |(e, a)| {
                    let mut rng =
                        Rng::for_entity(99, crate::sim::RngStream::COMBAT, tick, e.to_bits());
                    a.0 = a.0.wrapping_add(rng.below(100));
                });
            }
            world.hash_component::<A>()
        };
        let h1 = run(1);
        assert_eq!(h1, run(4));
        assert_eq!(h1, run(8));
    }
}
