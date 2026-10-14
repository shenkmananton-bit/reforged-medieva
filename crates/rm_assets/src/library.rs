//! Вся библиотека ассетов-заглушек, собранная за один вызов.
//!
//! Порядок сборки:
//!
//! 1. текстуры материалов → атлас материалов (`mat/<имя>`);
//! 2. маски грязи/снега/крови → атлас масок с **той же раскладкой**, что у
//!    атласа материалов: одни и те же UV читают и цвет, и маску (H-12);
//! 3. модели на трёх уровнях детализации, их UV указывают в атлас материалов;
//! 4. импосторы: самый подробный уровень запекается в картинку →
//!    атлас импосторов (`imp/<модель>`);
//! 5. LOD-цепочки: три меша + импостор, пороги из [`LodThresholds`].

use crate::atlas::{Atlas, AtlasBuilder};
use crate::error::{AssetError, Result};
use crate::image::Image;
use crate::impostor;
use crate::lod::{LodChain, LodGeometry, LodLevel, LodThresholds};
use crate::mesh::MaterialUvs;
use crate::models::{Category, MESH_LODS, ModelId};
use crate::pack::{Pack, PackWriter};
use crate::textures::{self, Material};

/// Настройки сборки.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BuildOptions {
    /// Зерно: одно и то же зерно — одни и те же байты пакета.
    pub seed: u32,
    /// Сторона текстуры одного материала, пикселей.
    pub material_size: u32,
    /// Рамка вокруг кусков атласа, пикселей.
    pub padding: u32,
    pub thresholds: LodThresholds,
}

impl Default for BuildOptions {
    fn default() -> Self {
        Self {
            seed: 1066,
            material_size: 128,
            padding: 4,
            thresholds: LodThresholds::default(),
        }
    }
}

/// Готовая библиотека.
#[derive(Debug, Clone, PartialEq)]
pub struct AssetLibrary {
    pub materials: Atlas,
    pub masks: Atlas,
    pub impostors: Atlas,
    pub models: Vec<(ModelId, LodChain)>,
}

/// Имена записей в пакете.
pub mod names {
    pub const MATERIALS: &str = "atlas/materials";
    pub const MASKS: &str = "atlas/masks";
    pub const IMPOSTORS: &str = "atlas/impostors";
    pub const MANIFEST: &str = "manifest";

    pub fn model(name: &str) -> String {
        format!("model/{name}")
    }
}

/// Плотность пикселей импостора и предел его стороны по группам моделей.
fn impostor_density(c: Category) -> (f32, u32) {
    match c {
        Category::Soldier => (24.0, 64),
        Category::Tree => (8.0, 64),
        Category::Building | Category::Wall => (4.0, 96),
    }
}

impl AssetLibrary {
    pub fn build(opt: &BuildOptions) -> Result<Self> {
        let size = opt.material_size;
        let mut mat = AtlasBuilder::new(opt.padding, 8192);
        let mut msk = AtlasBuilder::new(opt.padding, 8192);
        for m in Material::ALL {
            let name = format!("mat/{}", m.name());
            mat.add(name.clone(), textures::generate(m, size, opt.seed));
            msk.add(name, textures::generate_mask(m, size, opt.seed));
        }
        let materials = mat.build()?;
        let masks = msk.build()?;
        debug_assert_eq!(
            materials
                .regions
                .iter()
                .map(|r| (r.x, r.y))
                .collect::<Vec<_>>(),
            masks.regions.iter().map(|r| (r.x, r.y)).collect::<Vec<_>>()
        );
        let uvs = MaterialUvs::from_atlas(&materials)?;

        let mut meshes = Vec::with_capacity(ModelId::ALL.len());
        let mut imp = AtlasBuilder::new(1, 4096);
        let mut sizes = Vec::with_capacity(ModelId::ALL.len());
        for id in ModelId::ALL {
            let lods: Vec<_> = (0..MESH_LODS).map(|d| id.build(&uvs, d)).collect();
            let (ppm, max_px) = impostor_density(id.category());
            let source = if id == ModelId::Banner {
                impostor::side_view(&lods[0])
            } else {
                lods[0].clone()
            };
            let baked = impostor::bake(&source, &materials.image, ppm, max_px);
            imp.add(format!("imp/{}", id.name()), baked.image);
            sizes.push((baked.width, baked.height));
            meshes.push(lods);
        }
        let impostors = imp.build()?;

        let mut models = Vec::with_capacity(ModelId::ALL.len());
        for ((id, lods), (width, height)) in ModelId::ALL.into_iter().zip(meshes).zip(sizes) {
            let t = opt.thresholds.for_category(id.category());
            let mut levels: Vec<LodLevel> = lods
                .into_iter()
                .enumerate()
                .map(|(i, m)| LodLevel {
                    max_distance: t[i],
                    geometry: LodGeometry::Mesh(m),
                })
                .collect();
            let uv = impostors
                .uv(&format!("imp/{}", id.name()))
                .ok_or_else(|| AssetError::Invalid(format!("нет импостора {}", id.name())))?;
            levels.push(LodLevel {
                max_distance: t[3],
                geometry: LodGeometry::Impostor { width, height, uv },
            });
            let chain = LodChain { levels };
            chain
                .validate()
                .map_err(|e| AssetError::Invalid(format!("{}: {e}", id.name())))?;
            models.push((id, chain));
        }
        Ok(Self {
            materials,
            masks,
            impostors,
            models,
        })
    }

    pub fn model(&self, id: ModelId) -> Option<&LodChain> {
        self.models.iter().find(|(m, _)| *m == id).map(|(_, c)| c)
    }

    /// Короткое текстовое описание содержимого (кладётся в пакет как `manifest`).
    pub fn manifest(&self) -> String {
        let mut s = String::from("; Reforged Medieval: манифест пакета ассетов-заглушек\n");
        let atlas_line = |name: &str, a: &Atlas| {
            format!(
                "atlas {name} {}x{} regions {}\n",
                a.image.width,
                a.image.height,
                a.regions.len()
            )
        };
        s += &atlas_line(names::MATERIALS, &self.materials);
        s += &atlas_line(names::MASKS, &self.masks);
        s += &atlas_line(names::IMPOSTORS, &self.impostors);
        for (id, chain) in &self.models {
            let tris: Vec<String> = chain
                .levels
                .iter()
                .map(|l| format!("{}@{}", l.geometry.triangle_count(), l.max_distance))
                .collect();
            s += &format!("model {} tris {}\n", id.name(), tris.join(" "));
        }
        s
    }

    /// Пакет `.rmpak` со всей библиотекой.
    pub fn to_pack(&self) -> Result<Vec<u8>> {
        let mut w = PackWriter::new();
        w.add_text(names::MANIFEST, &self.manifest())?;
        w.add_atlas(names::MATERIALS, &self.materials)?;
        w.add_atlas(names::MASKS, &self.masks)?;
        w.add_atlas(names::IMPOSTORS, &self.impostors)?;
        for (id, chain) in &self.models {
            w.add_lod_chain(&names::model(id.name()), chain)?;
        }
        Ok(w.finish())
    }

    /// Обратное к [`Self::to_pack`].
    pub fn from_pack(bytes: &[u8]) -> Result<Self> {
        let pack = Pack::parse(bytes)?;
        let mut models = Vec::new();
        for id in ModelId::ALL {
            models.push((id, pack.lod_chain(&names::model(id.name()))?));
        }
        Ok(Self {
            materials: pack.atlas(names::MATERIALS)?,
            masks: pack.atlas(names::MASKS)?,
            impostors: pack.atlas(names::IMPOSTORS)?,
            models,
        })
    }

    /// Отдельная бесшовная текстура почвы (для texture splatting вне атласа).
    pub fn terrain_texture(m: Material, size: u32, seed: u32) -> Option<Image> {
        m.is_terrain().then(|| textures::generate(m, size, seed))
    }
}
