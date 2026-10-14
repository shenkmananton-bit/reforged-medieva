//! Равномерная сетка для поиска соседей.
//!
//! Солдату каждый шаг нужно знать, кто стоит рядом: с кем расталкиваться
//! и кого бить. Перебирать всех со всеми — это O(N²), при 10 000 солдат
//! сто миллионов пар. Поэтому поле режется на клетки по 4 метра, а соседей
//! ищем только в ближайших клетках.
//!
//! Сетка строится заново каждый шаг сортировкой подсчётом: без словарей,
//! без случайного порядка, одинаково при каждом запуске.

use crate::math::Vec2;

pub(crate) struct Grid {
    cell: f32,
    min: f32,
    dim: i32,
    /// `starts[k]..starts[k + 1]` — диапазон в `items` для клетки `k`.
    starts: Vec<u32>,
    items: Vec<u32>,
}

impl Grid {
    /// `half` — половина стороны поля; сетка берёт поле с запасом
    /// для убегающих за край, всё дальше попадает в крайние клетки.
    pub fn build(positions: &[Vec2], half: f32, cell: f32) -> Self {
        let min = -half - 64.0;
        let dim = (((half + 64.0) * 2.0) / cell).ceil() as i32 + 1;
        let mut grid = Self {
            cell,
            min,
            dim,
            starts: vec![0; (dim * dim) as usize + 1],
            items: vec![0; positions.len()],
        };
        let keys: Vec<usize> = positions.iter().map(|&p| grid.key(p)).collect();
        for &k in &keys {
            grid.starts[k + 1] += 1;
        }
        for k in 1..grid.starts.len() {
            grid.starts[k] += grid.starts[k - 1];
        }
        let mut cursor = grid.starts.clone();
        for (i, &k) in keys.iter().enumerate() {
            grid.items[cursor[k] as usize] = i as u32;
            cursor[k] += 1;
        }
        grid
    }

    fn coord(&self, v: f32) -> i32 {
        (((v - self.min) / self.cell) as i32).clamp(0, self.dim - 1)
    }

    fn key(&self, p: Vec2) -> usize {
        (self.coord(p.x) + self.coord(p.y) * self.dim) as usize
    }

    /// Вызывает `f` для каждого индекса в клетках, задевающих круг `(p, radius)`.
    /// Расстояние проверяет сам вызывающий.
    pub fn for_each_near(&self, p: Vec2, radius: f32, mut f: impl FnMut(usize)) {
        let (x0, x1) = (self.coord(p.x - radius), self.coord(p.x + radius));
        let (y0, y1) = (self.coord(p.y - radius), self.coord(p.y + radius));
        for cy in y0..=y1 {
            for cx in x0..=x1 {
                let k = (cx + cy * self.dim) as usize;
                for &i in &self.items[self.starts[k] as usize..self.starts[k + 1] as usize] {
                    f(i as usize);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_neighbours_and_skips_far_points() {
        let pts = [
            Vec2::new(0.0, 0.0),
            Vec2::new(1.0, 1.0),
            Vec2::new(50.0, 50.0),
            Vec2::new(-1000.0, 3.0), // за краем поля — в крайней клетке
        ];
        let grid = Grid::build(&pts, 100.0, 4.0);
        let mut near = Vec::new();
        grid.for_each_near(Vec2::new(0.5, 0.5), 2.0, |i| near.push(i));
        near.sort_unstable();
        assert_eq!(near, vec![0, 1]);
        let mut far = Vec::new();
        grid.for_each_near(Vec2::new(-170.0, 3.0), 2.0, |i| far.push(i));
        assert_eq!(far, vec![3]);
    }
}
