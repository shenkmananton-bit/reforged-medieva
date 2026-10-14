#version 450
// Низкополигональные модели с GPU instancing (хак H-13): один вызов
// рисует всех солдат одного типа. Данные экземпляра — позиция, поворот,
// масштаб, фаза анимации и два цвета фракции.
#extension GL_GOOGLE_include_directive : require
#include "common.glsl"

layout(location = 0) in vec3 a_pos;
layout(location = 1) in vec3 a_normal;
// rgb — цвет; a — откуда брать цвет: 0 — свой, 1 — основной цвет фракции, 2 — второй.
layout(location = 2) in vec4 a_color;

layout(location = 3) in vec4 i_pos_scale; // xyz — позиция, w — масштаб
layout(location = 4) in vec4 i_dir_anim;  // xy — направление взгляда (x, z), z — фаза шага, w — удар 0..1
layout(location = 5) in vec4 i_color1;    // основной цвет; a — прозрачность
layout(location = 6) in vec4 i_color2;    // второй цвет

layout(location = 0) out vec3 v_color;
layout(location = 1) out vec3 v_normal;
layout(location = 2) out float v_fog;
layout(location = 3) out float v_alpha;

void main() {
    vec3 p = a_pos;
    float walk = i_dir_anim.z;
    float strike = i_dir_anim.w;
    // Простая процедурная анимация вместо скелета: ноги качаются,
    // корпус подпрыгивает при ходьбе и наклоняется вперёд при ударе.
    if (p.y > 0.8) {
        p.y += abs(sin(walk)) * 0.05;
        p.z += strike * 0.35 * (p.y - 0.8);
    } else if (walk != 0.0) {
        p.z += sin(walk) * 0.25 * sign(p.x) * (0.8 - p.y);
    }

    vec3 fwd = vec3(i_dir_anim.x, 0.0, i_dir_anim.y);
    vec3 right = vec3(-i_dir_anim.y, 0.0, i_dir_anim.x);
    vec3 up = vec3(0.0, 1.0, 0.0);
    vec3 world = (right * p.x + up * p.y + fwd * p.z) * i_pos_scale.w + i_pos_scale.xyz;
    gl_Position = frame.view_proj * vec4(world, 1.0);

    vec3 c = a_color.rgb;
    if (a_color.a > 1.5) {
        c *= i_color2.rgb;
    } else if (a_color.a > 0.5) {
        c *= i_color1.rgb;
    }
    v_color = c;
    v_normal = right * a_normal.x + up * a_normal.y + fwd * a_normal.z;
    v_fog = fog_amount(world);
    v_alpha = i_color1.a;
}
