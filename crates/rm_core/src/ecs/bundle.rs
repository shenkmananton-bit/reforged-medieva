//! Bundle — набор компонентов, с которым сущность создаётся: `(Pos, Vel, Hp)`.

use std::mem::ManuallyDrop;

use super::component::{Component, ComponentId, ComponentRegistry};

/// Кортеж компонентов. Реализован для кортежей длиной от 0 до 12.
/// Один компонент передаётся как кортеж из одного элемента: `(Pos(0.0),)`.
///
/// # Safety
/// `register` должен перечислять типы ровно в том порядке, в котором `write`
/// отдаёт значения, и `write` должен отдать каждое значение ровно один раз.
pub unsafe trait Bundle: Send + Sync + 'static {
    /// Дописывает id типов в порядке полей кортежа.
    fn register(registry: &mut ComponentRegistry, out: &mut Vec<ComponentId>);

    /// Отдаёт каждое поле как `(номер поля, указатель на значение)`.
    /// Владение значением переходит получателю.
    ///
    /// # Safety
    /// Получатель обязан забрать (скопировать) каждое значение.
    unsafe fn write(self, sink: &mut impl FnMut(usize, *const u8));
}

macro_rules! impl_bundle {
    ($(($T:ident, $v:ident, $i:tt)),*) => {
        unsafe impl<$($T: Component),*> Bundle for ($($T,)*) {
            fn register(_registry: &mut ComponentRegistry, _out: &mut Vec<ComponentId>) {
                $( _out.push(_registry.register::<$T>()); )*
            }

            unsafe fn write(self, _sink: &mut impl FnMut(usize, *const u8)) {
                let ($($v,)*) = self;
                $(
                    let $v = ManuallyDrop::new($v);
                    _sink($i, (&*$v as *const $T).cast());
                )*
            }
        }
    };
}

impl_bundle!();
impl_bundle!((A, a, 0));
impl_bundle!((A, a, 0), (B, b, 1));
impl_bundle!((A, a, 0), (B, b, 1), (C, c, 2));
impl_bundle!((A, a, 0), (B, b, 1), (C, c, 2), (D, d, 3));
impl_bundle!((A, a, 0), (B, b, 1), (C, c, 2), (D, d, 3), (E, e, 4));
impl_bundle!(
    (A, a, 0),
    (B, b, 1),
    (C, c, 2),
    (D, d, 3),
    (E, e, 4),
    (F, f, 5)
);
impl_bundle!(
    (A, a, 0),
    (B, b, 1),
    (C, c, 2),
    (D, d, 3),
    (E, e, 4),
    (F, f, 5),
    (G, g, 6)
);
impl_bundle!(
    (A, a, 0),
    (B, b, 1),
    (C, c, 2),
    (D, d, 3),
    (E, e, 4),
    (F, f, 5),
    (G, g, 6),
    (H, h, 7)
);
impl_bundle!(
    (A, a, 0),
    (B, b, 1),
    (C, c, 2),
    (D, d, 3),
    (E, e, 4),
    (F, f, 5),
    (G, g, 6),
    (H, h, 7),
    (I, i, 8)
);
impl_bundle!(
    (A, a, 0),
    (B, b, 1),
    (C, c, 2),
    (D, d, 3),
    (E, e, 4),
    (F, f, 5),
    (G, g, 6),
    (H, h, 7),
    (I, i, 8),
    (J, j, 9)
);
impl_bundle!(
    (A, a, 0),
    (B, b, 1),
    (C, c, 2),
    (D, d, 3),
    (E, e, 4),
    (F, f, 5),
    (G, g, 6),
    (H, h, 7),
    (I, i, 8),
    (J, j, 9),
    (K, k, 10)
);
impl_bundle!(
    (A, a, 0),
    (B, b, 1),
    (C, c, 2),
    (D, d, 3),
    (E, e, 4),
    (F, f, 5),
    (G, g, 6),
    (H, h, 7),
    (I, i, 8),
    (J, j, 9),
    (K, k, 10),
    (L, l, 11)
);
