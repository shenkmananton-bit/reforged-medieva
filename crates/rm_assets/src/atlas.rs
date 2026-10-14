//! Упаковка многих маленьких текстур в одну большую (хак H-11).
//!
//! Рендер привязывает одну текстуру атласа и рисует все модели, у которых
//! UV-координаты уже указывают на нужный кусок. Сменить вид модели —
//! значит сменить координаты, а не текстуру и не draw call.
//!
//! Упаковщик «полками»: куски сортируются по высоте, кладутся слева
//! направо в ряд; ряд кончился — начинается новая полка ниже. Вокруг каждого
//! куска оставляется рамка (gutter), в которую растянуты его крайние пиксели:
//! без неё мип-уровни смешали бы соседние куски, и на модели проступил бы
//! чужой цвет по краям.

use crate::error::{AssetError, Result};
use crate::image::Image;

/// Прямоугольник в UV-координатах атласа: `u0, v0` — левый верхний угол.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct UvRect {
    pub u0: f32,
    pub v0: f32,
    pub u1: f32,
    pub v1: f32,
}

impl UvRect {
    /// Весь атлас.
    pub const FULL: UvRect = UvRect {
        u0: 0.0,
        v0: 0.0,
        u1: 1.0,
        v1: 1.0,
    };

    /// Переводит локальные координаты куска (`0..1`) в координаты атласа.
    pub fn map(&self, uv: [f32; 2]) -> [f32; 2] {
        [
            self.u0 + (self.u1 - self.u0) * uv[0],
            self.v0 + (self.v1 - self.v0) * uv[1],
        ]
    }

    pub fn contains(&self, uv: [f32; 2]) -> bool {
        uv[0] >= self.u0 && uv[0] <= self.u1 && uv[1] >= self.v0 && uv[1] <= self.v1
    }
}

/// Один кусок атласа.
#[derive(Debug, Clone, PartialEq)]
pub struct AtlasRegion {
    pub name: String,
    /// Положение в пикселях (без рамки).
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
    pub uv: UvRect,
}

/// Готовый атлас: картинка и таблица кусков.
#[derive(Debug, Clone, PartialEq)]
pub struct Atlas {
    pub image: Image,
    pub regions: Vec<AtlasRegion>,
}

impl Atlas {
    pub fn find(&self, name: &str) -> Option<&AtlasRegion> {
        self.regions.iter().find(|r| r.name == name)
    }

    pub fn uv(&self, name: &str) -> Option<UvRect> {
        self.find(name).map(|r| r.uv)
    }

    /// Вырезает кусок обратно в отдельную картинку (для проверок и превью).
    pub fn crop(&self, name: &str) -> Option<Image> {
        let r = self.find(name)?;
        Some(Image::from_fn(r.w, r.h, |x, y| {
            self.image.get(r.x + x, r.y + y)
        }))
    }

    /// UV-прямоугольник куска по его пикселям и размеру атласа.
    pub(crate) fn uv_for(x: u32, y: u32, w: u32, h: u32, aw: u32, ah: u32) -> UvRect {
        UvRect {
            u0: x as f32 / aw as f32,
            v0: y as f32 / ah as f32,
            u1: (x + w) as f32 / aw as f32,
            v1: (y + h) as f32 / ah as f32,
        }
    }
}

/// Собирает атлас из именованных картинок.
#[derive(Debug, Clone)]
pub struct AtlasBuilder {
    items: Vec<(String, Image)>,
    padding: u32,
    max_size: u32,
}

impl AtlasBuilder {
    /// `padding` — ширина рамки в пикселях, `max_size` — предел стороны атласа.
    pub fn new(padding: u32, max_size: u32) -> Self {
        Self {
            items: Vec::new(),
            padding,
            max_size,
        }
    }

    pub fn add(&mut self, name: impl Into<String>, image: Image) -> &mut Self {
        self.items.push((name.into(), image));
        self
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Упаковывает куски в наименьший подходящий прямоугольник со сторонами-степенями двойки.
    pub fn build(&self) -> Result<Atlas> {
        let mut names: Vec<&str> = self.items.iter().map(|(n, _)| n.as_str()).collect();
        names.sort_unstable();
        if let Some(w) = names.windows(2).find(|w| w[0] == w[1]) {
            return Err(AssetError::Atlas(format!("имя «{}» повторяется", w[0])));
        }
        if self.items.is_empty() {
            return Err(AssetError::Atlas("нет ни одной картинки".into()));
        }
        // Порядок упаковки: выше — раньше, при равной высоте шире — раньше,
        // затем по имени. Так результат не зависит от порядка add().
        let mut order: Vec<usize> = (0..self.items.len()).collect();
        order.sort_by(|&a, &b| {
            let (na, ia) = &self.items[a];
            let (nb, ib) = &self.items[b];
            ib.height
                .cmp(&ia.height)
                .then(ib.width.cmp(&ia.width))
                .then(na.cmp(nb))
        });
        let p = self.padding;
        let area: u64 = self
            .items
            .iter()
            .map(|(_, i)| (i.width + 2 * p) as u64 * (i.height + 2 * p) as u64)
            .sum();
        // Кандидаты по возрастанию площади: 1×1, 2×1, 2×2, 4×2, 4×4…
        // (ширина — степень двойки, высота — та же или вдвое меньше).
        let mut side = 1u32;
        while side <= self.max_size {
            for (w, h) in [(side, (side / 2).max(1)), (side, side)] {
                if (w as u64) * (h as u64) < area {
                    continue;
                }
                if let Some(places) = self.try_pack(&order, w, h) {
                    return Ok(self.compose(w, h, &places));
                }
            }
            side *= 2;
        }
        Err(AssetError::Atlas(format!(
            "{} картинок не помещаются в {}×{}",
            self.items.len(),
            self.max_size,
            self.max_size
        )))
    }

    /// Раскладывает куски по полкам; возвращает левый верхний угол каждого (с рамкой).
    fn try_pack(&self, order: &[usize], width: u32, height: u32) -> Option<Vec<(u32, u32)>> {
        let p = self.padding;
        let mut places = vec![(0, 0); self.items.len()];
        let (mut x, mut y, mut shelf_h) = (0u32, 0u32, 0u32);
        for &i in order {
            let img = &self.items[i].1;
            let (w, h) = (img.width + 2 * p, img.height + 2 * p);
            if w > width {
                return None;
            }
            if x + w > width {
                x = 0;
                y += shelf_h;
                shelf_h = 0;
            }
            if y + h > height {
                return None;
            }
            places[i] = (x, y);
            x += w;
            shelf_h = shelf_h.max(h);
        }
        Some(places)
    }

    fn compose(&self, width: u32, height: u32, places: &[(u32, u32)]) -> Atlas {
        let p = self.padding;
        let mut image = Image::new(width, height, [0, 0, 0, 0]);
        let mut regions = Vec::with_capacity(self.items.len());
        for ((name, img), &(px, py)) in self.items.iter().zip(places) {
            // Кусок вместе с рамкой: пиксель рамки берёт ближайший край куска.
            for y in 0..img.height + 2 * p {
                for x in 0..img.width + 2 * p {
                    let sx = x.saturating_sub(p).min(img.width - 1);
                    let sy = y.saturating_sub(p).min(img.height - 1);
                    image.set(px + x, py + y, img.get(sx, sy));
                }
            }
            let (x, y) = (px + p, py + p);
            regions.push(AtlasRegion {
                name: name.clone(),
                x,
                y,
                w: img.width,
                h: img.height,
                uv: Atlas::uv_for(x, y, img.width, img.height, width, height),
            });
        }
        regions.sort_by(|a, b| a.name.cmp(&b.name));
        Atlas { image, regions }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(w: u32, h: u32, c: u8) -> Image {
        Image::new(w, h, [c, c, c, 255])
    }

    #[test]
    fn regions_do_not_overlap_and_keep_pixels() {
        let mut b = AtlasBuilder::new(2, 1024);
        for i in 0..10u8 {
            b.add(
                format!("t{i}"),
                solid(8 + i as u32 * 3, 16 - i as u32, i * 20),
            );
        }
        let atlas = b.build().unwrap();
        for (i, a) in atlas.regions.iter().enumerate() {
            for b in &atlas.regions[i + 1..] {
                let apart = a.x + a.w + 2 <= b.x
                    || b.x + b.w + 2 <= a.x
                    || a.y + a.h + 2 <= b.y
                    || b.y + b.h + 2 <= a.y;
                assert!(apart, "{} и {} перекрываются", a.name, b.name);
            }
        }
        let t3 = atlas.crop("t3").unwrap();
        assert!(t3.pixels.iter().all(|p| *p == [60, 60, 60, 255]));
    }

    #[test]
    fn gutter_repeats_edge_pixels() {
        let mut b = AtlasBuilder::new(3, 64);
        b.add("a", solid(4, 4, 200));
        b.add("b", solid(4, 4, 10));
        let atlas = b.build().unwrap();
        let r = atlas.find("a").unwrap();
        assert_eq!(atlas.image.get(r.x - 3, r.y - 3), [200, 200, 200, 255]);
        assert_eq!(atlas.image.get(r.x + r.w + 2, r.y), [200, 200, 200, 255]);
    }

    #[test]
    fn result_does_not_depend_on_insertion_order() {
        let mut a = AtlasBuilder::new(1, 256);
        let mut b = AtlasBuilder::new(1, 256);
        let imgs: Vec<_> = (0..6)
            .map(|i| (format!("n{i}"), solid(10 + i, 7 + i, i as u8)))
            .collect();
        for (n, i) in &imgs {
            a.add(n.clone(), i.clone());
        }
        for (n, i) in imgs.iter().rev() {
            b.add(n.clone(), i.clone());
        }
        assert_eq!(a.build().unwrap(), b.build().unwrap());
    }

    #[test]
    fn errors_are_reported() {
        let mut dup = AtlasBuilder::new(0, 64);
        dup.add("x", solid(2, 2, 0)).add("x", solid(2, 2, 0));
        assert!(matches!(dup.build(), Err(AssetError::Atlas(_))));
        let mut big = AtlasBuilder::new(0, 16);
        big.add("huge", solid(32, 32, 0));
        assert!(big.build().is_err());
    }

    #[test]
    fn uv_map_hits_region() {
        let mut b = AtlasBuilder::new(1, 64);
        b.add("a", solid(8, 8, 1)).add("b", solid(8, 8, 2));
        let atlas = b.build().unwrap();
        let r = atlas.uv("b").unwrap();
        let mid = r.map([0.5, 0.5]);
        assert!(r.contains(mid));
        let px = atlas.image.sample(mid[0], mid[1]);
        assert_eq!(px, [2, 2, 2, 255]);
    }
}
