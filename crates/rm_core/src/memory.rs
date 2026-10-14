//! Линейные (bump) аллокаторы для временных данных.
//!
//! Идея: берём большой кусок памяти и выдаём из него «отрезки», просто
//! сдвигая указатель. Освобождать по одному нельзя — зато можно сбросить всё
//! разом одной операцией ([`Arena::reset`]). Это идеально для данных, живущих
//! один кадр: списки видимых юнитов, временные пути, команды отрисовки.
//!
//! Выделение — несколько арифметических операций вместо обращения к
//! системному аллокатору, и вся временная память кадра лежит компактно.

use std::alloc::{self, Layout};
use std::cell::{Cell, UnsafeCell};
use std::ptr::{self, NonNull};

/// Выравнивание начала каждого блока арены.
const CHUNK_ALIGN: usize = 16;

struct Chunk {
    ptr: NonNull<u8>,
    size: usize,
}

impl Chunk {
    fn new(size: usize) -> Self {
        let layout =
            Layout::from_size_align(size, CHUNK_ALIGN).expect("слишком большой блок арены");
        // SAFETY: size > 0 (гарантирует вызывающий).
        let ptr = NonNull::new(unsafe { alloc::alloc(layout) })
            .unwrap_or_else(|| alloc::handle_alloc_error(layout));
        Self { ptr, size }
    }
}

impl Drop for Chunk {
    fn drop(&mut self) {
        let layout = Layout::from_size_align(self.size, CHUNK_ALIGN).unwrap();
        unsafe { alloc::dealloc(self.ptr.as_ptr(), layout) }
    }
}

/// Bump-арена.
///
/// * Выделение через `&self`, поэтому можно держать много выданных ссылок одновременно.
/// * [`Arena::reset`] требует `&mut self` — компилятор не даст сбросить арену,
///   пока жива хоть одна выданная ссылка.
/// * Деструкторы (`Drop`) у размещённых значений **не вызываются**. Кладите
///   сюда простые данные (`Copy`-структуры, срезы чисел).
/// * Не `Sync`: у каждого потока своя арена.
pub struct Arena {
    /// Все блоки; последний — текущий. Блоки никогда не перемещаются.
    chunks: UnsafeCell<Vec<Chunk>>,
    /// Смещение свободного места в текущем блоке.
    offset: Cell<usize>,
    /// Байт выдано с последнего сброса (включая потери на выравнивание).
    used: Cell<usize>,
    /// Максимум `used` за всё время — полезно для настройки бюджетов памяти.
    peak: Cell<usize>,
}

impl Arena {
    /// Арена с начальным блоком в `capacity` байт. Если место кончится,
    /// будет выделен ещё блок, а при следующем `reset` они сольются в один.
    pub fn with_capacity(capacity: usize) -> Self {
        let capacity = capacity.max(CHUNK_ALIGN);
        Self {
            chunks: UnsafeCell::new(vec![Chunk::new(capacity)]),
            offset: Cell::new(0),
            used: Cell::new(0),
            peak: Cell::new(0),
        }
    }

    /// Выделяет сырую память под `layout`.
    pub fn alloc_layout(&self, layout: Layout) -> NonNull<u8> {
        if layout.size() == 0 {
            return NonNull::new(ptr::without_provenance_mut(layout.align())).unwrap();
        }
        // SAFETY: доступ к Vec только внутри методов арены, ссылки на него наружу не уходят;
        // Arena не Sync, так что параллельного доступа нет.
        let chunks = unsafe { &mut *self.chunks.get() };
        let chunk = chunks.last().unwrap();
        let base = chunk.ptr.as_ptr() as usize;
        let start = (base + self.offset.get()).next_multiple_of(layout.align()) - base;
        let end = start
            .checked_add(layout.size())
            .expect("переполнение размера");
        let (ptr, consumed) = if end <= chunk.size {
            let consumed = end - self.offset.get();
            self.offset.set(end);
            // SAFETY: start < chunk.size.
            (unsafe { chunk.ptr.as_ptr().add(start) }, consumed)
        } else {
            // Новый блок: минимум вдвое больше прошлого и с запасом под выравнивание.
            let size = (chunk.size * 2).max(layout.size() + layout.align());
            chunks.push(Chunk::new(size));
            let chunk = chunks.last().unwrap();
            let base = chunk.ptr.as_ptr() as usize;
            let start = base.next_multiple_of(layout.align()) - base;
            self.offset.set(start + layout.size());
            (
                unsafe { chunk.ptr.as_ptr().add(start) },
                start + layout.size(),
            )
        };
        let used = self.used.get() + consumed;
        self.used.set(used);
        self.peak.set(self.peak.get().max(used));
        // SAFETY: указатель внутри живого блока.
        unsafe { NonNull::new_unchecked(ptr) }
    }

    /// Кладёт значение в арену. `Drop` у него вызван не будет.
    #[allow(clippy::mut_from_ref)]
    pub fn alloc<T>(&self, value: T) -> &mut T {
        let ptr = self.alloc_layout(Layout::new::<T>()).cast::<T>();
        // SAFETY: память свежая, выровнена и принадлежит только этой ссылке.
        unsafe {
            ptr.as_ptr().write(value);
            &mut *ptr.as_ptr()
        }
    }

    /// Копирует срез в арену.
    #[allow(clippy::mut_from_ref)]
    pub fn alloc_slice_copy<T: Copy>(&self, src: &[T]) -> &mut [T] {
        let ptr = self.alloc_layout(Layout::for_value(src)).cast::<T>();
        // SAFETY: память свежая, размер под src.len() элементов.
        unsafe {
            ptr::copy_nonoverlapping(src.as_ptr(), ptr.as_ptr(), src.len());
            std::slice::from_raw_parts_mut(ptr.as_ptr(), src.len())
        }
    }

    /// Срез длины `len`, заполненный `f(0)..f(len-1)`.
    #[allow(clippy::mut_from_ref)]
    pub fn alloc_slice_fill_with<T>(&self, len: usize, mut f: impl FnMut(usize) -> T) -> &mut [T] {
        let layout = Layout::array::<T>(len).expect("переполнение размера");
        let ptr = self.alloc_layout(layout).cast::<T>();
        for i in 0..len {
            // SAFETY: i < len. Если f запаникует, уже записанные значения
            // просто останутся без Drop — это допустимо.
            unsafe { ptr.as_ptr().add(i).write(f(i)) };
        }
        // SAFETY: все len элементов инициализированы.
        unsafe { std::slice::from_raw_parts_mut(ptr.as_ptr(), len) }
    }

    /// Копирует строку в арену.
    #[allow(clippy::mut_from_ref)]
    pub fn alloc_str(&self, s: &str) -> &mut str {
        let bytes = self.alloc_slice_copy(s.as_bytes());
        // SAFETY: байты взяты из валидной UTF-8 строки.
        unsafe { std::str::from_utf8_unchecked_mut(bytes) }
    }

    /// Сбрасывает арену: вся выданная память снова свободна. Если за кадр
    /// понадобилось несколько блоков, они заменяются одним общим размером,
    /// чтобы в следующем кадре хватило одного.
    pub fn reset(&mut self) {
        let chunks = self.chunks.get_mut();
        if chunks.len() > 1 {
            let total: usize = chunks.iter().map(|c| c.size).sum();
            chunks.clear();
            chunks.push(Chunk::new(total));
        }
        self.offset.set(0);
        self.used.set(0);
    }

    /// Байт выдано с последнего `reset`.
    pub fn used(&self) -> usize {
        self.used.get()
    }

    /// Максимальное `used` за всё время жизни арены.
    pub fn peak(&self) -> usize {
        self.peak.get()
    }

    /// Сколько памяти арена держит у системы.
    pub fn capacity(&self) -> usize {
        // SAFETY: только чтение, наружу ссылка не уходит.
        unsafe { &*self.chunks.get() }.iter().map(|c| c.size).sum()
    }
}

impl Default for Arena {
    fn default() -> Self {
        Self::with_capacity(64 * 1024)
    }
}

/// Две арены, которые меняются местами каждый кадр.
///
/// Данные, записанные в кадре N в [`FrameArenas::current`], ещё доступны в
/// кадре N+1 через [`FrameArenas::previous`] (например, позиции прошлого тика
/// для интерполяции или команды, которые рендер-поток читает с отставанием в кадр),
/// а в начале кадра N+2 сбрасываются.
pub struct FrameArenas {
    arenas: [Arena; 2],
    current: usize,
}

impl FrameArenas {
    pub fn with_capacity(capacity_each: usize) -> Self {
        Self {
            arenas: [
                Arena::with_capacity(capacity_each),
                Arena::with_capacity(capacity_each),
            ],
            current: 0,
        }
    }

    /// Начало нового кадра: текущая арена становится прошлой, а позапрошлая
    /// сбрасывается и становится текущей.
    pub fn begin_frame(&mut self) {
        self.current ^= 1;
        self.arenas[self.current].reset();
    }

    pub fn current(&self) -> &Arena {
        &self.arenas[self.current]
    }

    pub fn previous(&self) -> &Arena {
        &self.arenas[self.current ^ 1]
    }
}

impl Default for FrameArenas {
    fn default() -> Self {
        Self::with_capacity(1024 * 1024)
    }
}

// Арену можно передать в другой поток целиком (например, своя арена на каждый
// рабочий поток), но не делить между потоками: Send есть, Sync нет.
// SAFETY: блоки памяти принадлежат арене эксклюзивно.
unsafe impl Send for Arena {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_are_aligned_and_distinct() {
        let arena = Arena::with_capacity(64);
        let a = arena.alloc(1u8);
        let b = arena.alloc(2u64);
        let c = arena.alloc([3u32; 4]);
        assert_eq!((b as *mut u64 as usize) % 8, 0);
        *a += 1;
        *b += 1;
        c[0] += 1;
        assert_eq!((*a, *b, c[0]), (2, 3, 4));
    }

    #[test]
    fn grows_and_merges_on_reset() {
        let mut arena = Arena::with_capacity(32);
        for i in 0..100u64 {
            assert_eq!(*arena.alloc(i), i);
        }
        assert!(arena.capacity() > 32);
        let cap = arena.capacity();
        arena.reset();
        assert_eq!(arena.used(), 0);
        assert_eq!(arena.capacity(), cap);
        for i in 0..100u64 {
            arena.alloc(i);
        }
        assert_eq!(
            arena.capacity(),
            cap,
            "после слияния всё помещается в один блок"
        );
        assert!(arena.peak() >= 800);
    }

    #[test]
    fn slices_and_strings() {
        let arena = Arena::default();
        let s = arena.alloc_slice_copy(&[1, 2, 3]);
        let f = arena.alloc_slice_fill_with(5, |i| i * i);
        let t = arena.alloc_str("Константинополь");
        let empty: &mut [u32] = arena.alloc_slice_copy(&[]);
        assert_eq!(s, &[1, 2, 3]);
        assert_eq!(f, &[0, 1, 4, 9, 16]);
        assert_eq!(t, "Константинополь");
        assert!(empty.is_empty());
    }

    #[test]
    fn big_alignment() {
        #[repr(align(64))]
        struct Big(u8);
        let arena = Arena::with_capacity(16);
        arena.alloc(1u8);
        let big = arena.alloc(Big(7));
        assert_eq!((big as *mut Big as usize) % 64, 0);
        assert_eq!(big.0, 7);
    }

    #[test]
    fn frame_arenas_keep_previous_frame() {
        let mut frames = FrameArenas::with_capacity(256);
        frames.current().alloc(1u32);
        let used = frames.current().used();
        frames.begin_frame();
        assert_eq!(frames.previous().used(), used);
        assert_eq!(frames.current().used(), 0);
        frames.begin_frame();
        assert_eq!(frames.previous().used(), 0);
    }
}
