//! # rm_campaign_map — как рисуется карта кампании
//!
//! Карта кампании Medieval II — не настоящий 3D-мир, а набор хитрых
//! приёмов, которые этот крейт повторяет (номера — из `docs/adr/smart-hacks.md`):
//!
//! * **H-01** псевдо-3D: лист, приподнятый по сетке высот ([`mesh`]);
//! * **H-02** свет и рельеф запечены заранее в lightmap и normal map ([`bake`]);
//! * **H-03** границы фракций рисуются через stencil-буфер ([`gpu`]);
//! * **H-04** цвет владельца берётся из маленькой таблицы: смена владельца
//!   региона — запись одного числа, вся карта — один вызов рисования ([`political`]);
//! * **H-05** горы вытянуты по высоте в 2,5 раза ([`map`]).
//!
//! Части крейта:
//! * [`map`] — процедурная карта-заглушка (рельеф, регионы, названия);
//! * [`political`] — кто какой регион держит, цвета фракций;
//! * [`bake`] — запекание текстур;
//! * [`mesh`] — сетка рельефа;
//! * [`camera`] — камера карты;
//! * [`gpu`] — всё про Vulkan: загрузка текстур и три прохода рисования.
//!
//! Всё, кроме [`gpu`], работает без видеокарты и проверяется тестами.
//! Подробное описание на русском — в `docs/campaign-map/`.

pub mod bake;
pub mod camera;
pub mod gpu;
pub mod grid;
pub mod map;
pub mod mesh;
mod noise;
pub mod political;

pub use bake::BakedMap;
pub use camera::MapCamera;
pub use map::{CampaignMap, MapConfig, Region, RegionId, SEA};
pub use mesh::TerrainMesh;
pub use political::{FactionIndex, FactionStyle, Political};
