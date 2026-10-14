//! Пакет ассетов `.rmpak`: один файл, внутри — именованные записи.
//!
//! Пакет — производный кэш (правило 4 из `docs/adr/modding-contract.md`):
//! он строится сборщиком `rm_assetc`, не лежит в репозитории и в любой
//! момент пересобирается. Формат описан в `docs/assets/formats.md`.
//!
//! Раскладка (все числа little-endian):
//!
//! ```text
//! "RMPK"  u16 версия  u16 флаги(0)  u32 число записей
//! оглавление: для каждой записи
//!     u8 вид  u16 длина имени  имя(UTF-8)  u64 смещение  u64 длина  u64 FNV-1a
//! данные записей подряд
//! ```

use crate::atlas::{Atlas, AtlasRegion, UvRect};
use crate::error::{AssetError, Result};
use crate::image::Image;
use crate::lod::{LodChain, LodGeometry, LodLevel};
use crate::mesh::{Mesh, Vertex};

pub const MAGIC: &[u8; 4] = b"RMPK";
pub const VERSION: u16 = 1;

/// Что лежит в записи.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    Image = 1,
    Mesh = 2,
    Atlas = 3,
    LodChain = 4,
    /// Произвольный текст UTF-8 (манифесты, описания).
    Text = 5,
}

impl EntryKind {
    fn from_u8(v: u8) -> Option<Self> {
        Some(match v {
            1 => EntryKind::Image,
            2 => EntryKind::Mesh,
            3 => EntryKind::Atlas,
            4 => EntryKind::LodChain,
            5 => EntryKind::Text,
            _ => return None,
        })
    }
}

/// Хеш FNV-1a 64: быстрый, без зависимостей, ловит повреждение данных.
pub fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

// ---------- Запись ----------

#[derive(Default)]
struct W(Vec<u8>);

impl W {
    fn u8(&mut self, v: u8) {
        self.0.push(v);
    }
    fn u16(&mut self, v: u16) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn u32(&mut self, v: u32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn u64(&mut self, v: u64) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn f32(&mut self, v: f32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn str(&mut self, s: &str) {
        self.u16(s.len() as u16);
        self.0.extend_from_slice(s.as_bytes());
    }
    fn image(&mut self, img: &Image) {
        self.u32(img.width);
        self.u32(img.height);
        for p in &img.pixels {
            self.0.extend_from_slice(p);
        }
    }
    fn mesh(&mut self, m: &Mesh) {
        self.u32(m.vertices.len() as u32);
        self.u32(m.indices.len() as u32);
        for v in &m.vertices {
            for c in v.pos.iter().chain(&v.normal).chain(&v.uv) {
                self.f32(*c);
            }
            self.f32(v.tint);
        }
        for &i in &m.indices {
            self.u32(i);
        }
    }
    fn uv(&mut self, r: &UvRect) {
        for c in [r.u0, r.v0, r.u1, r.v1] {
            self.f32(c);
        }
    }
}

/// Собирает пакет в памяти.
#[derive(Default)]
pub struct PackWriter {
    entries: Vec<(EntryKind, String, Vec<u8>)>,
}

impl PackWriter {
    pub fn new() -> Self {
        Self::default()
    }

    fn push(&mut self, kind: EntryKind, name: &str, data: Vec<u8>) -> Result<()> {
        if name.len() > u16::MAX as usize {
            return Err(AssetError::Pack(format!("слишком длинное имя «{name}»")));
        }
        if self.entries.iter().any(|(_, n, _)| n == name) {
            return Err(AssetError::Pack(format!("запись «{name}» уже есть")));
        }
        self.entries.push((kind, name.to_string(), data));
        Ok(())
    }

    pub fn add_image(&mut self, name: &str, img: &Image) -> Result<()> {
        let mut w = W::default();
        w.image(img);
        self.push(EntryKind::Image, name, w.0)
    }

    pub fn add_mesh(&mut self, name: &str, mesh: &Mesh) -> Result<()> {
        let mut w = W::default();
        w.mesh(mesh);
        self.push(EntryKind::Mesh, name, w.0)
    }

    pub fn add_atlas(&mut self, name: &str, atlas: &Atlas) -> Result<()> {
        let mut w = W::default();
        w.image(&atlas.image);
        w.u32(atlas.regions.len() as u32);
        for r in &atlas.regions {
            w.str(&r.name);
            for c in [r.x, r.y, r.w, r.h] {
                w.u32(c);
            }
        }
        self.push(EntryKind::Atlas, name, w.0)
    }

    pub fn add_lod_chain(&mut self, name: &str, chain: &LodChain) -> Result<()> {
        let mut w = W::default();
        w.u32(chain.levels.len() as u32);
        for l in &chain.levels {
            w.f32(l.max_distance);
            match &l.geometry {
                LodGeometry::Mesh(m) => {
                    w.u8(0);
                    w.mesh(m);
                }
                LodGeometry::Impostor { width, height, uv } => {
                    w.u8(1);
                    w.f32(*width);
                    w.f32(*height);
                    w.uv(uv);
                }
            }
        }
        self.push(EntryKind::LodChain, name, w.0)
    }

    pub fn add_text(&mut self, name: &str, text: &str) -> Result<()> {
        self.push(EntryKind::Text, name, text.as_bytes().to_vec())
    }

    /// Готовые байты файла.
    pub fn finish(self) -> Vec<u8> {
        let mut head = W::default();
        head.0.extend_from_slice(MAGIC);
        head.u16(VERSION);
        head.u16(0);
        head.u32(self.entries.len() as u32);
        let toc_len: usize = self
            .entries
            .iter()
            .map(|(_, n, _)| 1 + 2 + n.len() + 8 * 3)
            .sum();
        let mut offset = (head.0.len() + toc_len) as u64;
        for (kind, name, data) in &self.entries {
            head.u8(*kind as u8);
            head.str(name);
            head.u64(offset);
            head.u64(data.len() as u64);
            head.u64(fnv1a(data));
            offset += data.len() as u64;
        }
        for (_, _, data) in &self.entries {
            head.0.extend_from_slice(data);
        }
        head.0
    }
}

// ---------- Чтение ----------

struct R<'a> {
    b: &'a [u8],
    pos: usize,
}

impl<'a> R<'a> {
    fn new(b: &'a [u8]) -> Self {
        Self { b, pos: 0 }
    }
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self
            .pos
            .checked_add(n)
            .filter(|&e| e <= self.b.len())
            .ok_or_else(|| AssetError::Pack("данные обрываются".into()))?;
        let s = &self.b[self.pos..end];
        self.pos = end;
        Ok(s)
    }
    fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }
    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn u64(&mut self) -> Result<u64> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }
    fn f32(&mut self) -> Result<f32> {
        Ok(f32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn str(&mut self) -> Result<String> {
        let n = self.u16()? as usize;
        String::from_utf8(self.take(n)?.to_vec())
            .map_err(|_| AssetError::Pack("имя записи не UTF-8".into()))
    }
    /// Сколько элементов размером `size` можно ещё прочитать — защита от
    /// огромных счётчиков в повреждённом файле.
    fn count(&mut self, size: usize) -> Result<usize> {
        let n = self.u32()? as usize;
        if n.saturating_mul(size) > self.b.len() - self.pos {
            return Err(AssetError::Pack(format!("счётчик {n} больше данных")));
        }
        Ok(n)
    }
    fn image(&mut self) -> Result<Image> {
        let w = self.u32()?;
        let h = self.u32()?;
        let n = (w as usize)
            .checked_mul(h as usize)
            .filter(|n| n.saturating_mul(4) <= self.b.len() - self.pos)
            .ok_or_else(|| AssetError::Pack(format!("размер картинки {w}×{h} больше данных")))?;
        let raw = self.take(n * 4)?;
        Ok(Image {
            width: w,
            height: h,
            pixels: raw
                .chunks_exact(4)
                .map(|c| [c[0], c[1], c[2], c[3]])
                .collect(),
        })
    }
    fn mesh(&mut self) -> Result<Mesh> {
        let nv = self.count(36)?;
        let ni = self.u32()? as usize;
        let mut vertices = Vec::with_capacity(nv);
        for _ in 0..nv {
            let mut f = [0.0f32; 9];
            for c in &mut f {
                *c = self.f32()?;
            }
            vertices.push(Vertex {
                pos: [f[0], f[1], f[2]],
                normal: [f[3], f[4], f[5]],
                uv: [f[6], f[7]],
                tint: f[8],
            });
        }
        if ni.saturating_mul(4) > self.b.len() - self.pos {
            return Err(AssetError::Pack(format!(
                "счётчик индексов {ni} больше данных"
            )));
        }
        let mut indices = Vec::with_capacity(ni);
        for _ in 0..ni {
            indices.push(self.u32()?);
        }
        let mesh = Mesh { vertices, indices };
        mesh.validate()?;
        Ok(mesh)
    }
}

/// Запись в оглавлении пакета.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub kind: EntryKind,
    pub name: String,
    pub offset: u64,
    pub len: u64,
}

/// Прочитанный пакет: оглавление и ссылка на байты файла.
#[derive(Debug)]
pub struct Pack<'a> {
    bytes: &'a [u8],
    pub entries: Vec<Entry>,
}

impl<'a> Pack<'a> {
    /// Разбирает оглавление и проверяет хеш каждой записи.
    pub fn parse(bytes: &'a [u8]) -> Result<Self> {
        let mut r = R::new(bytes);
        if r.take(4).ok() != Some(MAGIC.as_slice()) {
            return Err(AssetError::Pack(
                "это не файл .rmpak (нет подписи RMPK)".into(),
            ));
        }
        let version = r.u16()?;
        if version != VERSION {
            return Err(AssetError::Pack(format!(
                "версия {version}, а сборка читает версию {VERSION}; пересоберите пакет rm_assetc"
            )));
        }
        let _flags = r.u16()?;
        let n = r.count(1 + 2 + 24)?;
        let mut entries = Vec::with_capacity(n);
        for _ in 0..n {
            let kind_raw = r.u8()?;
            let kind = EntryKind::from_u8(kind_raw)
                .ok_or_else(|| AssetError::Pack(format!("неизвестный вид записи {kind_raw}")))?;
            let name = r.str()?;
            let offset = r.u64()?;
            let len = r.u64()?;
            let hash = r.u64()?;
            let data = offset
                .checked_add(len)
                .filter(|&e| e <= bytes.len() as u64)
                .map(|e| &bytes[offset as usize..e as usize])
                .ok_or_else(|| {
                    AssetError::Pack(format!("запись «{name}» выходит за конец файла"))
                })?;
            if fnv1a(data) != hash {
                return Err(AssetError::Pack(format!(
                    "запись «{name}» повреждена (не сходится хеш)"
                )));
            }
            entries.push(Entry {
                kind,
                name,
                offset,
                len,
            });
        }
        Ok(Self { bytes, entries })
    }

    pub fn find(&self, name: &str) -> Option<&Entry> {
        self.entries.iter().find(|e| e.name == name)
    }

    fn data(&self, name: &str, kind: EntryKind) -> Result<&'a [u8]> {
        let e = self
            .find(name)
            .ok_or_else(|| AssetError::Pack(format!("нет записи «{name}»")))?;
        if e.kind != kind {
            return Err(AssetError::Pack(format!(
                "запись «{name}» — {:?}, а ожидался {kind:?}",
                e.kind
            )));
        }
        Ok(&self.bytes[e.offset as usize..(e.offset + e.len) as usize])
    }

    pub fn image(&self, name: &str) -> Result<Image> {
        R::new(self.data(name, EntryKind::Image)?).image()
    }

    pub fn mesh(&self, name: &str) -> Result<Mesh> {
        R::new(self.data(name, EntryKind::Mesh)?).mesh()
    }

    pub fn text(&self, name: &str) -> Result<String> {
        String::from_utf8(self.data(name, EntryKind::Text)?.to_vec())
            .map_err(|_| AssetError::Pack(format!("запись «{name}» не UTF-8")))
    }

    pub fn atlas(&self, name: &str) -> Result<Atlas> {
        let mut r = R::new(self.data(name, EntryKind::Atlas)?);
        let image = r.image()?;
        let n = r.count(2 + 16)?;
        let mut regions = Vec::with_capacity(n);
        for _ in 0..n {
            let rname = r.str()?;
            let (x, y, w, h) = (r.u32()?, r.u32()?, r.u32()?, r.u32()?);
            if x.checked_add(w).is_none_or(|e| e > image.width)
                || y.checked_add(h).is_none_or(|e| e > image.height)
            {
                return Err(AssetError::Pack(format!("кусок «{rname}» вне атласа")));
            }
            regions.push(AtlasRegion {
                name: rname,
                x,
                y,
                w,
                h,
                uv: Atlas::uv_for(x, y, w, h, image.width, image.height),
            });
        }
        Ok(Atlas { image, regions })
    }

    pub fn lod_chain(&self, name: &str) -> Result<LodChain> {
        let mut r = R::new(self.data(name, EntryKind::LodChain)?);
        let n = r.count(5)?;
        let mut levels = Vec::with_capacity(n);
        for _ in 0..n {
            let max_distance = r.f32()?;
            let geometry = match r.u8()? {
                0 => LodGeometry::Mesh(r.mesh()?),
                1 => LodGeometry::Impostor {
                    width: r.f32()?,
                    height: r.f32()?,
                    uv: UvRect {
                        u0: r.f32()?,
                        v0: r.f32()?,
                        u1: r.f32()?,
                        v1: r.f32()?,
                    },
                },
                k => return Err(AssetError::Pack(format!("неизвестный вид уровня LOD {k}"))),
            };
            levels.push(LodLevel {
                max_distance,
                geometry,
            });
        }
        let chain = LodChain { levels };
        chain.validate()?;
        Ok(chain)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mesh::MaterialUvs;
    use crate::models::ModelId;

    fn sample() -> Vec<u8> {
        let uvs = MaterialUvs::identity();
        let mut w = PackWriter::new();
        w.add_image("img", &Image::new(3, 2, [1, 2, 3, 4])).unwrap();
        w.add_mesh("mesh", &ModelId::House.build(&uvs, 1)).unwrap();
        w.add_text("readme", "привет").unwrap();
        w.finish()
    }

    #[test]
    fn round_trip() {
        let bytes = sample();
        let pack = Pack::parse(&bytes).unwrap();
        assert_eq!(pack.entries.len(), 3);
        assert_eq!(pack.image("img").unwrap(), Image::new(3, 2, [1, 2, 3, 4]));
        let uvs = MaterialUvs::identity();
        assert_eq!(pack.mesh("mesh").unwrap(), ModelId::House.build(&uvs, 1));
        assert_eq!(pack.text("readme").unwrap(), "привет");
    }

    #[test]
    fn corruption_is_detected() {
        let mut bytes = sample();
        let last = bytes.len() - 1;
        bytes[last] ^= 0xff;
        let err = Pack::parse(&bytes).unwrap_err().to_string();
        assert!(err.contains("повреждена"), "{err}");

        let mut bytes = sample();
        bytes[4] = 99;
        assert!(
            Pack::parse(&bytes)
                .unwrap_err()
                .to_string()
                .contains("версия")
        );

        assert!(Pack::parse(b"nope").is_err());
        let bytes = sample();
        for cut in [3, 11, 20, bytes.len() - 5] {
            assert!(Pack::parse(&bytes[..cut]).is_err(), "обрезка {cut}");
        }
    }

    #[test]
    fn wrong_kind_and_missing_names() {
        let bytes = sample();
        let pack = Pack::parse(&bytes).unwrap();
        assert!(pack.mesh("img").is_err());
        assert!(pack.image("nothing").is_err());
        let mut w = PackWriter::new();
        w.add_text("a", "1").unwrap();
        assert!(w.add_text("a", "2").is_err());
    }
}
