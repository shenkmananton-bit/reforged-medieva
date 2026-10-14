//! Сборка всей библиотеки: правила, которые проверяются на готовых ассетах.

use rm_assets::lod::LodGeometry;
use rm_assets::{AssetLibrary, BuildOptions, ModelId};

fn small() -> BuildOptions {
    BuildOptions {
        material_size: 32,
        ..BuildOptions::default()
    }
}

#[test]
fn library_round_trips_through_pack() {
    let lib = AssetLibrary::build(&small()).unwrap();
    let bytes = lib.to_pack().unwrap();
    let back = AssetLibrary::from_pack(&bytes).unwrap();
    assert_eq!(lib, back);
}

#[test]
fn build_is_deterministic() {
    let a = AssetLibrary::build(&small()).unwrap().to_pack().unwrap();
    let b = AssetLibrary::build(&small()).unwrap().to_pack().unwrap();
    assert_eq!(a, b);
    let other = BuildOptions { seed: 7, ..small() };
    let c = AssetLibrary::build(&other).unwrap().to_pack().unwrap();
    assert_ne!(a, c);
}

#[test]
fn every_vertex_samples_inside_one_atlas_region() {
    let lib = AssetLibrary::build(&small()).unwrap();
    for (id, chain) in &lib.models {
        for level in &chain.levels {
            let LodGeometry::Mesh(mesh) = &level.geometry else {
                continue;
            };
            for tri in mesh.indices.as_chunks::<3>().0 {
                let uv0 = mesh.vertices[tri[0] as usize].uv;
                let region = lib
                    .materials
                    .regions
                    .iter()
                    .find(|r| r.uv.contains(uv0))
                    .unwrap_or_else(|| panic!("{}: UV {uv0:?} вне атласа", id.name()));
                for &i in &tri[1..] {
                    assert!(region.uv.contains(mesh.vertices[i as usize].uv));
                }
            }
        }
    }
}

#[test]
fn masks_share_layout_with_materials() {
    let lib = AssetLibrary::build(&small()).unwrap();
    assert_eq!(lib.materials.image.width, lib.masks.image.width);
    for (a, b) in lib.materials.regions.iter().zip(&lib.masks.regions) {
        assert_eq!((&a.name, a.x, a.y, a.w, a.h), (&b.name, b.x, b.y, b.w, b.h));
    }
}

#[test]
fn every_model_has_four_levels_ending_with_impostor() {
    let lib = AssetLibrary::build(&small()).unwrap();
    assert_eq!(lib.models.len(), ModelId::ALL.len());
    for (id, chain) in &lib.models {
        assert_eq!(chain.levels.len(), 4, "{}", id.name());
        assert!(matches!(
            chain.levels[3].geometry,
            LodGeometry::Impostor { .. }
        ));
        let region = lib.impostors.find(&format!("imp/{}", id.name())).unwrap();
        let img = lib.impostors.crop(&region.name).unwrap();
        assert!(
            img.pixels.iter().any(|p| p[3] != 0),
            "{}: пустой импостор",
            id.name()
        );
    }
    assert!(lib.manifest().contains("model spearman"));
}
