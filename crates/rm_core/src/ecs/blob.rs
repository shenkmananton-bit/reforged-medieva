//! `BlobVec` — «стёртый по типу» `Vec`: непрерывный буфер элементов одного
//! (известного только во время выполнения) размера. Это одна колонка архетипа.

use std::alloc::{self, Layout};
use std::ptr::{self, NonNull};

pub(crate) struct BlobVec {
    item_layout: Layout,
    drop_fn: Option<unsafe fn(*mut u8)>,
    data: NonNull<u8>,
    len: usize,
    cap: usize,
}

// SAFETY: в BlobVec кладутся только компоненты, а они по определению
// `Send + Sync` (см. `Component`).
unsafe impl Send for BlobVec {}
unsafe impl Sync for BlobVec {}

impl BlobVec {
    pub(crate) fn new(item_layout: Layout, drop_fn: Option<unsafe fn(*mut u8)>) -> Self {
        // Для типов нулевого размера (теги) память не нужна вообще.
        let cap = if item_layout.size() == 0 {
            usize::MAX
        } else {
            0
        };
        Self {
            item_layout,
            drop_fn,
            data: dangling(item_layout.align()),
            len: 0,
            cap,
        }
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.len
    }

    #[inline]
    pub(crate) fn as_ptr(&self) -> *mut u8 {
        self.data.as_ptr()
    }

    /// Указатель на элемент `row`.
    ///
    /// # Safety
    /// `row <= cap` (указатель на `len` допустим как «место для следующего»).
    #[inline]
    pub(crate) unsafe fn ptr_at(&self, row: usize) -> *mut u8 {
        unsafe { self.data.as_ptr().add(row * self.item_layout.size()) }
    }

    /// # Safety
    /// `T` — тот самый тип, под который создана колонка, и `row < len`.
    #[inline]
    pub(crate) unsafe fn get<T>(&self, row: usize) -> &T {
        debug_assert!(row < self.len);
        unsafe { &*self.ptr_at(row).cast::<T>() }
    }

    /// # Safety
    /// Как у [`Self::get`].
    #[inline]
    pub(crate) unsafe fn get_mut<T>(&mut self, row: usize) -> &mut T {
        debug_assert!(row < self.len);
        unsafe { &mut *self.ptr_at(row).cast::<T>() }
    }

    /// Гарантирует место ещё под `additional` элементов.
    pub(crate) fn reserve(&mut self, additional: usize) {
        let needed = self
            .len
            .checked_add(additional)
            .expect("переполнение ёмкости колонки");
        if needed <= self.cap {
            return;
        }
        let new_cap = needed.max(self.cap * 2).max(4);
        let new_layout = self.array_layout(new_cap);
        // SAFETY: размер элемента ненулевой (иначе cap == usize::MAX и мы бы вышли выше).
        let new_ptr = unsafe {
            if self.cap == 0 {
                alloc::alloc(new_layout)
            } else {
                alloc::realloc(
                    self.data.as_ptr(),
                    self.array_layout(self.cap),
                    new_layout.size(),
                )
            }
        };
        self.data = NonNull::new(new_ptr).unwrap_or_else(|| alloc::handle_alloc_error(new_layout));
        self.cap = new_cap;
    }

    /// Копирует байты значения из `src` в конец колонки. Владение значением
    /// переходит колонке; вызывающий не должен его дропать.
    ///
    /// # Safety
    /// `src` указывает на валидное значение типа колонки; место уже
    /// зарезервировано через [`Self::reserve`].
    #[inline]
    pub(crate) unsafe fn push_raw(&mut self, src: *const u8) {
        debug_assert!(self.len < self.cap);
        unsafe {
            ptr::copy_nonoverlapping(src, self.ptr_at(self.len), self.item_layout.size());
        }
        self.len += 1;
    }

    /// Удаляет строку `row` перестановкой с последней, *не вызывая* `Drop`.
    /// Удалённое значение остаётся лежать сразу за концом колонки (по адресу
    /// [`Self::removed_ptr`]) до следующего `push`. Вызывающий обязан либо
    /// перенести его, либо вызвать [`Self::drop_removed`].
    ///
    /// Такое разделение нужно, чтобы пользовательский `Drop` выполнялся только
    /// когда все колонки уже согласованы — даже паника в нём не ломает мир.
    ///
    /// # Safety
    /// `row < len`.
    #[inline]
    pub(crate) unsafe fn swap_remove_keep(&mut self, row: usize) {
        debug_assert!(row < self.len);
        let last = self.len - 1;
        if row != last {
            unsafe {
                ptr::swap_nonoverlapping(
                    self.ptr_at(row),
                    self.ptr_at(last),
                    self.item_layout.size(),
                );
            }
        }
        self.len = last;
    }

    /// Адрес значения, удалённого последним [`Self::swap_remove_keep`].
    #[inline]
    pub(crate) fn removed_ptr(&self) -> *mut u8 {
        // SAFETY: len < cap сразу после swap_remove_keep.
        unsafe { self.ptr_at(self.len) }
    }

    /// Вызывает `Drop` у значения, удалённого последним [`Self::swap_remove_keep`].
    ///
    /// # Safety
    /// Вызывается ровно один раз после `swap_remove_keep`, значение не было перенесено.
    #[inline]
    pub(crate) unsafe fn drop_removed(&mut self) {
        if let Some(drop_fn) = self.drop_fn {
            unsafe { drop_fn(self.removed_ptr()) }
        }
    }

    fn array_layout(&self, n: usize) -> Layout {
        let size = self
            .item_layout
            .size()
            .checked_mul(n)
            .expect("переполнение размера колонки");
        Layout::from_size_align(size, self.item_layout.align()).expect("неверная раскладка колонки")
    }
}

impl Drop for BlobVec {
    fn drop(&mut self) {
        let len = self.len;
        // Сначала обнуляем длину: если чей-то Drop запаникует, оставшиеся
        // значения просто утекут, но двойного освобождения не будет.
        self.len = 0;
        if let Some(drop_fn) = self.drop_fn {
            for row in 0..len {
                unsafe { drop_fn(self.ptr_at(row)) }
            }
        }
        if self.item_layout.size() != 0 && self.cap != 0 {
            unsafe { alloc::dealloc(self.data.as_ptr(), self.array_layout(self.cap)) }
        }
    }
}

fn dangling(align: usize) -> NonNull<u8> {
    NonNull::new(ptr::without_provenance_mut(align)).expect("выравнивание не может быть нулём")
}
