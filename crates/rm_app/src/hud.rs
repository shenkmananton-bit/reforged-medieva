//! Построитель интерфейса: прямоугольники и текст в пикселях окна.
//!
//! Всё складывается в один список [`UiRect`] и рисуется одним вызовом
//! (хак H-25). Текст — это пиксельный шрифт из `font.rs`, каждая точка
//! буквы — отдельный маленький прямоугольник.

use crate::font::{self, GLYPH_H, GLYPH_W};
use crate::render::UiRect;

pub type Color = [f32; 4];

pub const WHITE: Color = [1.0, 1.0, 1.0, 1.0];
pub const SHADOW: Color = [0.0, 0.0, 0.0, 0.6];

pub struct Hud<'a> {
    pub rects: &'a mut Vec<UiRect>,
}

impl Hud<'_> {
    pub fn rect(&mut self, x: f32, y: f32, w: f32, h: f32, color: Color) {
        self.rects.push(UiRect {
            rect: [x, y, w, h],
            color,
        });
    }

    /// Рамка толщиной `t` внутри прямоугольника.
    pub fn frame(&mut self, x: f32, y: f32, w: f32, h: f32, t: f32, color: Color) {
        self.rect(x, y, w, t, color);
        self.rect(x, y + h - t, w, t, color);
        self.rect(x, y + t, t, h - 2.0 * t, color);
        self.rect(x + w - t, y + t, t, h - 2.0 * t, color);
    }

    /// Текст, левый верхний угол в (x, y); `scale` — пикселей на точку шрифта.
    pub fn text(&mut self, x: f32, y: f32, text: &str, scale: f32, color: Color) {
        let mut cx = x;
        for c in text.chars() {
            let rows = font::glyph(c);
            for (row, line) in rows.iter().enumerate() {
                // Соседние точки строки склеиваем в один прямоугольник.
                let bytes = line.as_bytes();
                let mut col = 0;
                while col < GLYPH_W {
                    if bytes[col] == b'#' {
                        let start = col;
                        while col < GLYPH_W && bytes[col] == b'#' {
                            col += 1;
                        }
                        self.rect(
                            cx + start as f32 * scale,
                            y + row as f32 * scale,
                            (col - start) as f32 * scale,
                            scale,
                            color,
                        );
                    } else {
                        col += 1;
                    }
                }
            }
            cx += (GLYPH_W + 1) as f32 * scale;
        }
    }

    /// Текст с тенью: читается на любом фоне.
    pub fn text_shadow(&mut self, x: f32, y: f32, text: &str, scale: f32, color: Color) {
        let d = (scale * 0.5).max(1.0);
        self.text(x + d, y + d, text, scale, SHADOW);
        self.text(x, y, text, scale, color);
    }

    /// Текст, выровненный по центру относительно `cx`.
    pub fn text_centered(&mut self, cx: f32, y: f32, text: &str, scale: f32, color: Color) {
        let w = text_width(text, scale);
        self.text_shadow(cx - w * 0.5, y, text, scale, color);
    }
}

pub fn text_width(text: &str, scale: f32) -> f32 {
    font::text_width(text) as f32 * scale
}

pub fn text_height(scale: f32) -> f32 {
    GLYPH_H as f32 * scale
}

/// Разбивает текст на строки не длиннее `max` символов по пробелам.
pub fn wrap(text: &str, max: usize) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for word in text.split_whitespace() {
        match lines.last_mut() {
            Some(line) if line.chars().count() + 1 + word.chars().count() <= max => {
                line.push(' ');
                line.push_str(word);
            }
            _ => lines.push(word.to_string()),
        }
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wraps_by_words() {
        assert_eq!(
            wrap("Shield Wall Spearmen", 12),
            vec!["Shield Wall", "Spearmen"]
        );
        assert_eq!(
            wrap("Steppe Horse Archers", 12),
            vec!["Steppe Horse", "Archers"]
        );
    }

    #[test]
    fn text_merges_runs_of_dots() {
        let mut rects = Vec::new();
        Hud { rects: &mut rects }.text(0.0, 0.0, "-", 2.0, WHITE);
        // «-» — одна строка из трёх точек подряд: один прямоугольник.
        assert_eq!(rects.len(), 1);
        assert_eq!(rects[0].rect, [2.0, 6.0, 6.0, 2.0]);
    }
}
