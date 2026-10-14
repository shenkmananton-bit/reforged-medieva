//! Картинка карты без видеокарты: `cargo run -p rm_campaign_map --release --example preview -- [зерно] [файл.ppm]`.
//!
//! Складывает запечённые albedo и lightmap так же, как шейдер (без
//! normal map и границ), и сохраняет в PPM — его открывают GIMP, IrfanView,
//! Paint.NET и большинство просмотрщиков. Удобно, чтобы быстро посмотреть
//! на карту с другим зерном.

use std::io::Write;

use rm_campaign_map::{BakedMap, CampaignMap, MapConfig};

fn main() -> std::io::Result<()> {
    let mut args = std::env::args().skip(1);
    let seed = args.next().and_then(|s| s.parse().ok()).unwrap_or(1);
    let path = args.next().unwrap_or_else(|| "campaign_map.ppm".into());
    let map = CampaignMap::generate(&MapConfig {
        seed,
        ..MapConfig::default()
    });
    let baked = BakedMap::bake(&map);
    let n = map.config.size as usize;
    let mut out = std::io::BufWriter::new(std::fs::File::create(&path)?);
    writeln!(out, "P6 {n} {n} 255")?;
    let albedo = &baked.albedo.levels[0];
    let light = &baked.light.levels[0];
    for i in 0..n * n {
        let shadow = f32::from(light[i * 2]) / 255.0;
        let ao = f32::from(light[i * 2 + 1]) / 255.0;
        let k = (0.45 + 0.75 * shadow) * ao;
        for c in 0..3 {
            out.write_all(&[(f32::from(albedo[i * 4 + c]) * k).min(255.0) as u8])?;
        }
    }
    println!(
        "{path}: {n}×{n}, регионов {}, зерно {seed}",
        map.land_regions().len()
    );
    Ok(())
}
