//! Картинка RGBA8 в памяти и чтение/запись формата TGA.
//!
//! TGA выбран потому, что Medieval II хранит в нём карты кампании
//! (`map_heights.tga`, `map_regions.tga` и другие): моддер сможет открыть
//! наши текстуры тем же редактором, а мы — прочитать его файлы.

use crate::error::{AssetError, Result};

/// Цвет пикселя: красный, зелёный, синий, альфа.
pub type Rgba = [u8; 4];

/// Картинка RGBA8, строки идут сверху вниз.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<Rgba>,
}

impl Image {
    /// Картинка, залитая одним цветом.
    pub fn new(width: u32, height: u32, fill: Rgba) -> Self {
        Self {
            width,
            height,
            pixels: vec![fill; width as usize * height as usize],
        }
    }

    /// Картинка, каждый пиксель которой считает функция `f(x, y)`.
    pub fn from_fn(width: u32, height: u32, mut f: impl FnMut(u32, u32) -> Rgba) -> Self {
        let mut pixels = Vec::with_capacity(width as usize * height as usize);
        for y in 0..height {
            for x in 0..width {
                pixels.push(f(x, y));
            }
        }
        Self {
            width,
            height,
            pixels,
        }
    }

    fn index(&self, x: u32, y: u32) -> usize {
        y as usize * self.width as usize + x as usize
    }

    pub fn get(&self, x: u32, y: u32) -> Rgba {
        self.pixels[self.index(x, y)]
    }

    pub fn set(&mut self, x: u32, y: u32, c: Rgba) {
        let i = self.index(x, y);
        self.pixels[i] = c;
    }

    /// Пиксель по нормированным координатам `u, v` в `[0, 1]`, ближайший сосед.
    pub fn sample(&self, u: f32, v: f32) -> Rgba {
        let x = ((u.clamp(0.0, 1.0) * self.width as f32) as u32).min(self.width - 1);
        let y = ((v.clamp(0.0, 1.0) * self.height as f32) as u32).min(self.height - 1);
        self.get(x, y)
    }

    /// Копирует `src` так, чтобы его левый верхний угол оказался в `(x, y)`.
    pub fn blit(&mut self, src: &Image, x: u32, y: u32) {
        for sy in 0..src.height {
            for sx in 0..src.width {
                self.set(x + sx, y + sy, src.get(sx, sy));
            }
        }
    }

    /// Средний цвет картинки (нужен для импосторов и мелких LOD).
    pub fn average(&self) -> Rgba {
        let mut sum = [0u64; 4];
        for p in &self.pixels {
            for c in 0..4 {
                sum[c] += p[c] as u64;
            }
        }
        let n = self.pixels.len().max(1) as u64;
        [
            (sum[0] / n) as u8,
            (sum[1] / n) as u8,
            (sum[2] / n) as u8,
            (sum[3] / n) as u8,
        ]
    }

    /// Уменьшает картинку вдвое по каждой стороне усреднением 2×2.
    pub fn half(&self) -> Image {
        let w = (self.width / 2).max(1);
        let h = (self.height / 2).max(1);
        Image::from_fn(w, h, |x, y| {
            let mut sum = [0u32; 4];
            for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                let sx = (x * 2 + dx).min(self.width - 1);
                let sy = (y * 2 + dy).min(self.height - 1);
                let p = self.get(sx, sy);
                for c in 0..4 {
                    sum[c] += p[c] as u32;
                }
            }
            [
                (sum[0] / 4) as u8,
                (sum[1] / 4) as u8,
                (sum[2] / 4) as u8,
                (sum[3] / 4) as u8,
            ]
        })
    }

    /// Цепочка мип-уровней от исходного размера до 1×1.
    pub fn mip_chain(&self) -> Vec<Image> {
        let mut chain = vec![self.clone()];
        while let Some(last) = chain.last() {
            if last.width == 1 && last.height == 1 {
                break;
            }
            let next = last.half();
            chain.push(next);
        }
        chain
    }

    /// Записывает несжатый 32-битный TGA (тип 2, начало координат сверху слева).
    pub fn to_tga(&self) -> Vec<u8> {
        assert!(self.width <= u16::MAX as u32 && self.height <= u16::MAX as u32);
        let mut out = Vec::with_capacity(18 + self.pixels.len() * 4);
        out.extend_from_slice(&[0, 0, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        out.extend_from_slice(&(self.width as u16).to_le_bytes());
        out.extend_from_slice(&(self.height as u16).to_le_bytes());
        // 32 бита на пиксель; дескриптор: 8 бит альфы + бит 5 «строки сверху вниз».
        out.extend_from_slice(&[32, 0x28]);
        for p in &self.pixels {
            out.extend_from_slice(&[p[2], p[1], p[0], p[3]]);
        }
        out
    }

    /// Читает TGA: несжатый (тип 2) или RLE (тип 10), 24 или 32 бита,
    /// также 8-битный серый (типы 3 и 11) — в нём часто лежат карты высот.
    pub fn from_tga(bytes: &[u8]) -> Result<Image> {
        let err = |m: &str| AssetError::Tga(m.to_string());
        if bytes.len() < 18 {
            return Err(err("файл короче заголовка (18 байт)"));
        }
        let id_len = bytes[0] as usize;
        let cmap_type = bytes[1];
        let kind = bytes[2];
        let width = u16::from_le_bytes([bytes[12], bytes[13]]) as u32;
        let height = u16::from_le_bytes([bytes[14], bytes[15]]) as u32;
        let bpp = bytes[16];
        let top_down = bytes[17] & 0x20 != 0;
        if cmap_type != 0 {
            return Err(err("палитровые TGA не поддерживаются"));
        }
        let (rle, gray) = match kind {
            2 => (false, false),
            3 => (false, true),
            10 => (true, false),
            11 => (true, true),
            _ => return Err(err(&format!("тип изображения {kind} не поддерживается"))),
        };
        let px_size = match (gray, bpp) {
            (false, 24) => 3,
            (false, 32) => 4,
            (true, 8) => 1,
            _ => return Err(err(&format!("{bpp} бит на пиксель не поддерживается"))),
        };
        if width == 0 || height == 0 {
            return Err(err("нулевой размер"));
        }
        let decode = |p: &[u8]| -> Rgba {
            match px_size {
                1 => [p[0], p[0], p[0], 255],
                3 => [p[2], p[1], p[0], 255],
                _ => [p[2], p[1], p[0], p[3]],
            }
        };
        let total = width as usize * height as usize;
        let mut pixels = Vec::with_capacity(total);
        let mut pos = 18 + id_len;
        let truncated = || err("данные пикселей обрываются");
        while pixels.len() < total {
            if rle {
                let head = *bytes.get(pos).ok_or_else(truncated)?;
                pos += 1;
                let count = (head & 0x7f) as usize + 1;
                if head & 0x80 != 0 {
                    let p = bytes.get(pos..pos + px_size).ok_or_else(truncated)?;
                    pos += px_size;
                    let c = decode(p);
                    for _ in 0..count {
                        pixels.push(c);
                    }
                } else {
                    for _ in 0..count {
                        let p = bytes.get(pos..pos + px_size).ok_or_else(truncated)?;
                        pos += px_size;
                        pixels.push(decode(p));
                    }
                }
            } else {
                let p = bytes.get(pos..pos + px_size).ok_or_else(truncated)?;
                pos += px_size;
                pixels.push(decode(p));
            }
        }
        if pixels.len() > total {
            return Err(err("RLE-пакет выходит за границу картинки"));
        }
        let mut img = Image {
            width,
            height,
            pixels,
        };
        if !top_down {
            // Классический TGA хранит строки снизу вверх; переворачиваем.
            let w = width as usize;
            let rows: Vec<Rgba> = img
                .pixels
                .chunks(w)
                .rev()
                .flat_map(|r| r.iter().copied())
                .collect();
            img.pixels = rows;
        }
        Ok(img)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tga_round_trip() {
        let img = Image::from_fn(5, 3, |x, y| [x as u8 * 40, y as u8 * 80, 7, 200]);
        let back = Image::from_tga(&img.to_tga()).unwrap();
        assert_eq!(img, back);
    }

    #[test]
    fn reads_bottom_up_gray_rle() {
        // 2×2 серый RLE снизу вверх: нижняя строка 10,10 (повтор), верхняя 1,2 (сырые).
        let mut f = vec![0, 0, 11, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2, 0, 2, 0, 8, 0];
        f.extend_from_slice(&[0x81, 10, 0x01, 1, 2]);
        let img = Image::from_tga(&f).unwrap();
        assert_eq!(img.get(0, 0), [1, 1, 1, 255]);
        assert_eq!(img.get(1, 0), [2, 2, 2, 255]);
        assert_eq!(img.get(0, 1), [10, 10, 10, 255]);
    }

    #[test]
    fn broken_tga_is_an_error() {
        assert!(Image::from_tga(&[0; 10]).is_err());
        let mut f = Image::new(4, 4, [1, 2, 3, 4]).to_tga();
        f.truncate(30);
        assert!(Image::from_tga(&f).is_err());
    }

    #[test]
    fn mip_chain_ends_at_one_pixel() {
        let chain = Image::new(8, 2, [0; 4]).mip_chain();
        let sizes: Vec<_> = chain.iter().map(|m| (m.width, m.height)).collect();
        assert_eq!(sizes, vec![(8, 2), (4, 1), (2, 1), (1, 1)]);
    }
}
