#version 450
// Проход 1: вся карта одним вызовом. Свет — из запечённых текстур (H-02),
// цвет владельца — из таблицы по номеру региона (H-04, texture splatting).
#extension GL_GOOGLE_include_directive : require
#define MAP_FRAGMENT
#include "map_common.glsl"

layout(location = 0) in vec3 v_world;
layout(location = 1) in vec2 v_uv;
layout(location = 0) out vec4 o_color;

const vec3 AMBIENT = vec3(0.42, 0.45, 0.52);
const vec3 SUN_COLOUR = vec3(1.0, 0.94, 0.82);
const vec3 HAZE = vec3(0.55, 0.62, 0.70);
// Цвет за краем карты; совпадает с цветом очистки экрана в rm_app/src/campaign.rs.
const vec3 OUTSIDE = vec3(0.0029, 0.0152, 0.0612);

// Мелкая «зернистость» земли вблизи: шум от хеша, без текстур. Так при
// сильном приближении земля не выглядит размытым пятном (в оригинале для
// этого были повторяющиеся detail-текстуры).
float hash2(vec2 p) {
    uvec2 q = uvec2(ivec2(p)) * uvec2(1597334673u, 3812015801u);
    uint n = (q.x ^ q.y) * 1597334673u;
    return float(n >> 8) / 16777216.0;
}

float detail_noise(vec2 p) {
    vec2 i = floor(p);
    vec2 f = fract(p);
    f = f * f * (3.0 - 2.0 * f);
    float a = hash2(i), b = hash2(i + vec2(1.0, 0.0));
    float c = hash2(i + vec2(0.0, 1.0)), d = hash2(i + vec2(1.0, 1.0));
    return mix(mix(a, b, f.x), mix(c, d, f.x), f.y);
}

vec3 hash_colour(uint id) {
    uint h = id * 2654435761u;
    return vec3((h >> 8) & 255u, (h >> 16) & 255u, (h >> 24) & 255u) / 255.0;
}

void main() {
    // Производные (fwidth) считаются до любых ветвлений.
    Quad q = gather_regions(v_uv);
    float owner_share = same_share(q, true);
    float region_share = same_share(q, false);
    float owner_px = line_distance_px(owner_share);
    float region_px = line_distance_px(region_share);

    vec4 albedo = texture(t_albedo, v_uv);
    vec3 n = normalize(texture(t_normal, v_uv).xyz * 2.0 - 1.0);
    vec2 lm = texture(t_light, v_uv).rg; // тень, затенение впадин
    float border_dist = texture(t_border, v_uv).r * BORDER_RANGE;
    bool water = albedo.a < 0.5;
    float ndl = max(dot(n, normalize(frame.sun.xyz)), 0.0);
    float light = water ? 0.55 + 0.45 * lm.r : 1.0;
    vec3 lit = water ? albedo.rgb * light
                     : albedo.rgb * (AMBIENT * lm.g + SUN_COLOUR * ndl * lm.r);

    // Цвет владельца: смешиваем цвета четырёх ближайших точек по весам,
    // чтобы переход между странами был плавным, а не «лесенкой».
    vec3 tint = vec3(0.0);
    float land = 0.0;
    float selected = 0.0;
    float hovered = 0.0;
    for (int k = 0; k < 4; k++) {
        if (q.id[k] != 0u) {
            tint += tables.colour[tables.owner[q.id[k]]].rgb * q.w[k];
            land += q.w[k];
        }
        selected += q.id[k] == frame.ids.x ? q.w[k] : 0.0;
        hovered += q.id[k] == frame.ids.y ? q.w[k] : 0.0;
    }
    uint view = frame.ids.z;
    uint flags = frame.ids.w;

    if (view == VIEW_ALBEDO) {
        o_color = vec4(albedo.rgb, 1.0);
        return;
    }
    if (view == VIEW_LIGHTMAP) {
        o_color = vec4(vec3(lm.r * lm.g), 1.0);
        return;
    }
    if (view == VIEW_NORMALS) {
        o_color = vec4(n * 0.5 + 0.5, 1.0);
        return;
    }

    if (land > 0.0 && !water) {
        tint /= land;
        float strength = frame.sun.w;
        float grey = dot(AMBIENT * lm.g + SUN_COLOUR * ndl * lm.r, vec3(0.33));
        if (view == VIEW_REGIONS) {
            uint nearest = q.id[0];
            float wbest = -1.0;
            for (int k = 0; k < 4; k++) if (q.w[k] > wbest && q.id[k] != 0u) { wbest = q.w[k]; nearest = q.id[k]; }
            tint = hash_colour(nearest);
            strength = 0.85;
        } else if (view == VIEW_POLITICAL) {
            strength = 0.8;
        }
        lit = mix(lit, tint * (0.25 + 0.85 * grey), strength * land);

        // Полоса цвета владельца вдоль внутренней стороны границы страны
        // (из запечённого поля расстояний).
        if ((flags & FLAG_GLOW) != 0u) {
            float d = border_dist;
            uint other = uint(texelFetch(t_border, ivec2(v_uv * frame.map.y), 0).g * 255.0 + 0.5);
            uint me = texelFetch(t_region, ivec2(v_uv * frame.map.y), 0).r;
            if (other != 0u && me != 0u && tables.owner[other] != tables.owner[me]) {
                float g = 1.0 - smoothstep(0.0, BORDER_RANGE, d);
                lit = mix(lit, tables.colour[tables.owner[me]].rgb * (0.4 + 0.8 * grey), g * g * 0.55);
            }
        }

        // Тонкая пунктирная линия между регионами одной страны. Границы
        // между странами рисует отдельный проход через stencil (H-03).
        if ((flags & FLAG_REGION_LINES) != 0u && owner_share > 0.999) {
            float dash = step(0.45, fract((v_world.x + v_world.z) * 0.35));
            float a = (1.0 - smoothstep(0.4, 1.2, region_px)) * dash;
            lit = mix(lit, lit * 0.45, a * 0.8);
        }
    }

    // Зернистость вблизи плавно исчезает с расстоянием.
    float near = 1.0 - smoothstep(60.0, 220.0, distance(v_world, frame.eye.xyz));
    if (!water && near > 0.0 && view == VIEW_FINAL) {
        vec2 p = v_uv * frame.map.y;
        float grain = detail_noise(p * 3.0) * 0.6 + detail_noise(p * 9.0) * 0.4;
        lit *= mix(1.0, 0.82 + 0.36 * grain, near);
    }

    // Выбранный регион светлее и чуть «дышит», под курсором — просто светлее.
    float pulse = 0.5 + 0.5 * sin(frame.eye.w * 3.0);
    lit *= 1.0 + selected * (0.22 + 0.12 * pulse) + hovered * 0.12;

    // Дымка вдали: далёкие горы бледнеют, как в оригинале.
    float dist = distance(v_world, frame.eye.xyz);
    float haze = smoothstep(frame.map.w, frame.map.w * 2.5, dist) * 0.55;
    lit = mix(lit, HAZE, haze);
    // У краёв карты море плавно уходит в цвет фона: не видно, где кончается сетка.
    vec2 e = min(v_uv, 1.0 - v_uv);
    float edge = 1.0 - smoothstep(0.0, 0.15, min(e.x, e.y));
    o_color = vec4(mix(lit, OUTSIDE, edge), 1.0);
}
