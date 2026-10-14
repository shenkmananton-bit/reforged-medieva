//! Выгрузка меша в Wavefront OBJ для просмотра.
//!
//! OBJ открывается почти любым 3D-просмотрщиком (Blender, «Средство
//! просмотра 3D» в Windows), поэтому сборщик кладёт его рядом с пакетом:
//! так можно глазами проверить модели, не запуская игру.

use std::fmt::Write;

use crate::mesh::Mesh;

/// Текст файла `.obj`. `mtl` — имя файла материалов, `material` — имя материала в нём.
pub fn to_obj(mesh: &Mesh, mtl: &str, material: &str) -> String {
    let mut s = String::new();
    let _ = writeln!(s, "# Reforged Medieval, процедурная модель-заглушка");
    let _ = writeln!(s, "mtllib {mtl}");
    let _ = writeln!(s, "usemtl {material}");
    for v in &mesh.vertices {
        let _ = writeln!(s, "v {} {} {}", v.pos[0], v.pos[1], v.pos[2]);
    }
    for v in &mesh.vertices {
        // В OBJ ось v идёт снизу вверх, у нас — сверху вниз.
        let _ = writeln!(s, "vt {} {}", v.uv[0], 1.0 - v.uv[1]);
    }
    for v in &mesh.vertices {
        let _ = writeln!(s, "vn {} {} {}", v.normal[0], v.normal[1], v.normal[2]);
    }
    for t in mesh.indices.as_chunks::<3>().0 {
        let (a, b, c) = (t[0] + 1, t[1] + 1, t[2] + 1);
        let _ = writeln!(s, "f {a}/{a}/{a} {b}/{b}/{b} {c}/{c}/{c}");
    }
    s
}

/// Текст файла `.mtl` с одним материалом, текстура которого — `texture` (TGA).
pub fn to_mtl(material: &str, texture: &str) -> String {
    format!("newmtl {material}\nKa 1 1 1\nKd 1 1 1\nmap_Kd {texture}\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mesh::MaterialUvs;
    use crate::models::ModelId;

    #[test]
    fn obj_has_all_parts() {
        let mesh = ModelId::Pine.build(&MaterialUvs::identity(), 2);
        let obj = to_obj(&mesh, "a.mtl", "atlas");
        let count = |p: &str| obj.lines().filter(|l| l.starts_with(p)).count();
        assert_eq!(count("v "), mesh.vertices.len());
        assert_eq!(count("vt "), mesh.vertices.len());
        assert_eq!(count("f "), mesh.triangle_count());
        assert!(to_mtl("atlas", "t.tga").contains("map_Kd t.tga"));
    }
}
