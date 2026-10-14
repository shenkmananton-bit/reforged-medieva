//! # rm_assets — конвейер ассетов Reforged Medieval
//!
//! Оригинальные модели и текстуры Medieval II в репозиторий не попадают,
//! поэтому все ассеты здесь — процедурные low-poly заглушки, собранные
//! кодом. Главное в крейте не красота, а форматы и приёмы, на которых
//! стоит рендер оригинала:
//!
//! * [`textures`] — процедурные материалы и маски грязи/снега/крови (H-12);
//! * [`atlas`] — упаковка текстур в атлас, рендер меняет только UV (H-11);
//! * [`mesh`], [`models`] — low-poly модели солдат, построек, стен, деревьев;
//! * [`lod`], [`impostor`] — LOD-цепочки с жёсткими порогами и импосторы (H-15);
//! * [`heightbake`] — запечённые normal map и lightmap из карты высот (H-02, H-05);
//! * [`image`] — картинки и формат TGA (как у карт кампании M2);
//! * [`pack`] — пакет `.rmpak`, [`obj`] — выгрузка в OBJ для просмотра;
//! * [`library`] — вся библиотека заглушек одним вызовом.
//!
//! Офлайн-сборщик — программа `rm_assetc` (`src/bin/rm_assetc.rs`).
//! Подробное описание на русском — в `docs/assets/`.

pub mod atlas;
pub mod error;
pub mod heightbake;
pub mod image;
pub mod impostor;
pub mod library;
pub mod lod;
pub mod mesh;
pub mod models;
pub mod noise;
pub mod obj;
pub mod pack;
pub mod textures;

pub use atlas::{Atlas, AtlasBuilder, UvRect};
pub use error::{AssetError, Result};
pub use image::Image;
pub use library::{AssetLibrary, BuildOptions};
pub use lod::{LodChain, LodGeometry, LodThresholds};
pub use mesh::{Mesh, Tint, Vertex};
pub use models::ModelId;
pub use textures::Material;
