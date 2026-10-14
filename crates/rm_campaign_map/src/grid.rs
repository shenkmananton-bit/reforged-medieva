//! Квадратная сетка значений: высоты, номера регионов, текстуры.
//!
//! Точка (x, y) сетки лежит в `data[y * size + x]`. Ось x идёт на восток,
//! ось y — на юг (как строки картинки), см. `docs/campaign-map/coordinates.md`.

#[derive(Debug, Clone, PartialEq)]
pub struct Grid<T> {
    pub size: u32,
    pub data: Vec<T>,
}

impl<T: Copy> Grid<T> {
    pub fn new(size: u32, fill: T) -> Self {
        Self {
            size,
            data: vec![fill; (size * size) as usize],
        }
    }

    #[inline]
    pub fn index(&self, x: u32, y: u32) -> usize {
        (y * self.size + x) as usize
    }

    #[inline]
    pub fn get(&self, x: u32, y: u32) -> T {
        self.data[self.index(x, y)]
    }

    #[inline]
    pub fn set(&mut self, x: u32, y: u32, v: T) {
        let i = self.index(x, y);
        self.data[i] = v;
    }

    /// Значение с прижатием координат к краю (за краем — крайняя точка).
    #[inline]
    pub fn get_clamped(&self, x: i64, y: i64) -> T {
        let m = i64::from(self.size) - 1;
        self.get(x.clamp(0, m) as u32, y.clamp(0, m) as u32)
    }
}

impl Grid<f32> {
    /// Значение между точками сетки (билинейная интерполяция).
    /// `x`, `y` — в точках сетки, дробные.
    pub fn bilinear(&self, x: f32, y: f32) -> f32 {
        let (fx, fy) = (x.floor(), y.floor());
        let (tx, ty) = (x - fx, y - fy);
        let (ix, iy) = (fx as i64, fy as i64);
        let a = self.get_clamped(ix, iy);
        let b = self.get_clamped(ix + 1, iy);
        let c = self.get_clamped(ix, iy + 1);
        let d = self.get_clamped(ix + 1, iy + 1);
        let top = a + (b - a) * tx;
        let bottom = c + (d - c) * tx;
        top + (bottom - top) * ty
    }
}

/// Делит строки сетки между потоками процессора. `f(y, row)` заполняет одну строку.
/// Запекание текстур — независимая работа на каждый пиксель, поэтому
/// оно хорошо делится на части.
pub fn par_rows<T: Send>(size: u32, out: &mut [T], f: impl Fn(u32, &mut [T]) + Sync) {
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get());
    let rows_per = (size as usize).div_ceil(threads).max(1);
    let row_len = size as usize;
    std::thread::scope(|s| {
        for (chunk_index, chunk) in out.chunks_mut(rows_per * row_len).enumerate() {
            let f = &f;
            s.spawn(move || {
                for (r, row) in chunk.chunks_mut(row_len).enumerate() {
                    f((chunk_index * rows_per + r) as u32, row);
                }
            });
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bilinear_hits_nodes_and_midpoints() {
        let mut g = Grid::new(2, 0.0f32);
        g.set(1, 0, 2.0);
        g.set(1, 1, 2.0);
        assert_eq!(g.bilinear(0.0, 0.0), 0.0);
        assert_eq!(g.bilinear(1.0, 1.0), 2.0);
        assert_eq!(g.bilinear(0.5, 0.5), 1.0);
        // За краем — прижато.
        assert_eq!(g.bilinear(5.0, -3.0), 2.0);
    }

    #[test]
    fn par_rows_fills_every_row() {
        let size = 37;
        let mut out = vec![0u32; 37 * 37];
        par_rows(size, &mut out, |y, row| row.fill(y + 1));
        for y in 0..size {
            assert!(
                out[(y * size) as usize..][..size as usize]
                    .iter()
                    .all(|&v| v == y + 1)
            );
        }
    }
}
