#version 450
// Проход 2 (H-03): маска границ стран в stencil-буфере. Цвет не пишется
// вовсе; фрагменты не на границе выбрасываются, а оставшиеся записывают
// в stencil единицу (это настроено в конвейере, не в шейдере).
// Глубина сравнивается на равенство: маска лежит ровно на рельефе и
// не видна сквозь горы.
#extension GL_GOOGLE_include_directive : require
#define MAP_FRAGMENT
#include "map_common.glsl"

layout(location = 0) in vec3 v_world;
layout(location = 1) in vec2 v_uv;

void main() {
    Quad q = gather_regions(v_uv);
    float d = line_distance_px(same_share(q, true));
    if ((frame.ids.w & FLAG_FACTION_BORDERS) == 0u || d > frame.map.z) {
        discard;
    }
}
