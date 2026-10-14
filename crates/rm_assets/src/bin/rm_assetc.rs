//! rm_assetc — офлайн-сборщик ассетов Reforged Medieval.
//!
//! ```text
//! rm_assetc build [--out ПАПКА] [--seed ЧИСЛО] [--size ПИКСЕЛИ] [--preview]
//! rm_assetc info ФАЙЛ.rmpak
//! ```
//!
//! `build` собирает пакет `assets.rmpak`; с `--preview` кладёт рядом атласы
//! в TGA, модели в OBJ и пример запечённой карты, чтобы посмотреть глазами.
//! Подробнее: docs/assets/rm_assetc.md.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use rm_assets::heightbake::{self, Heightmap};
use rm_assets::lod::LodGeometry;
use rm_assets::pack::Pack;
use rm_assets::{AssetLibrary, BuildOptions, obj};

type Res<T> = Result<T, Box<dyn std::error::Error>>;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("build") => build(&args[1..]),
        Some("info") => match args.get(1) {
            Some(p) => info(Path::new(p)),
            None => Err("укажите файл: rm_assetc info assets.rmpak".into()),
        },
        _ => {
            eprintln!("{}", USAGE);
            return ExitCode::from(2);
        }
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("ошибка: {e}");
            ExitCode::FAILURE
        }
    }
}

const USAGE: &str = "rm_assetc — сборщик ассетов Reforged Medieval

  rm_assetc build [--out ПАПКА] [--seed ЧИСЛО] [--size ПИКСЕЛИ] [--preview]
      собрать assets.rmpak (по умолчанию в папку assets/);
      --preview — ещё атласы .tga, модели .obj и пример карты высот
  rm_assetc info ФАЙЛ.rmpak
      показать, что лежит в пакете";

fn build(args: &[String]) -> Res<()> {
    let mut out = PathBuf::from("assets");
    let mut opt = BuildOptions::default();
    let mut preview = false;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut value = || it.next().ok_or_else(|| format!("после {a} нужно значение"));
        match a.as_str() {
            "--out" => out = PathBuf::from(value()?),
            "--seed" => opt.seed = value()?.parse()?,
            "--size" => opt.material_size = value()?.parse()?,
            "--preview" => preview = true,
            other => return Err(format!("неизвестный ключ {other}\n\n{USAGE}").into()),
        }
    }
    if !opt.material_size.is_power_of_two() || !(8..=1024).contains(&opt.material_size) {
        return Err("--size должен быть степенью двойки от 8 до 1024".into());
    }
    let started = std::time::Instant::now();
    let lib = AssetLibrary::build(&opt)?;
    std::fs::create_dir_all(&out)?;
    let bytes = lib.to_pack()?;
    let pak = out.join("assets.rmpak");
    std::fs::write(&pak, &bytes)?;
    println!(
        "собран {} ({} КБ) за {} мс",
        pak.display(),
        bytes.len() / 1024,
        started.elapsed().as_millis()
    );
    if preview {
        write_preview(&lib, &out, opt.seed)?;
    }
    Ok(())
}

fn write_preview(lib: &AssetLibrary, out: &Path, seed: u32) -> Res<()> {
    let dir = out.join("preview");
    std::fs::create_dir_all(&dir)?;
    std::fs::write(dir.join("materials.tga"), lib.materials.image.to_tga())?;
    std::fs::write(dir.join("masks.tga"), lib.masks.image.to_tga())?;
    std::fs::write(dir.join("impostors.tga"), lib.impostors.image.to_tga())?;
    std::fs::write(
        dir.join("materials.mtl"),
        obj::to_mtl("atlas", "materials.tga"),
    )?;
    let mut count = 0;
    for (id, chain) in &lib.models {
        for (i, level) in chain.levels.iter().enumerate() {
            if let LodGeometry::Mesh(m) = &level.geometry {
                let name = format!("{}_lod{i}.obj", id.name());
                std::fs::write(dir.join(name), obj::to_obj(m, "materials.mtl", "atlas"))?;
                count += 1;
            }
        }
    }
    // Пример H-02/H-05: карта высот, горы ×2,5, normal map и lightmap.
    let mut hm = Heightmap::procedural(256, 256, 400.0, seed);
    hm.exaggerate_mountains(150.0, 2.5);
    std::fs::write(
        dir.join("demo_normals.tga"),
        heightbake::normal_map(&hm, 40.0).to_tga(),
    )?;
    std::fs::write(
        dir.join("demo_lightmap.tga"),
        heightbake::lightmap(&hm, 40.0, [-0.7, 0.3, -0.5]).to_tga(),
    )?;
    println!(
        "превью: {} ({count} моделей OBJ, атласы и карты TGA)",
        dir.display()
    );
    Ok(())
}

fn info(path: &Path) -> Res<()> {
    let bytes = std::fs::read(path)?;
    let pack = Pack::parse(&bytes)?;
    println!("{}: {} записей", path.display(), pack.entries.len());
    for e in &pack.entries {
        println!(
            "  {:<10} {:<24} {:>9} байт",
            format!("{:?}", e.kind),
            e.name,
            e.len
        );
    }
    if pack.find("manifest").is_some() {
        println!("\n{}", pack.text("manifest")?);
    }
    Ok(())
}
