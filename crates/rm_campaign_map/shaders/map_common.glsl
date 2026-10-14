// Общее для всех шейдеров карты: данные кадра, текстуры и таблицы владельцев.
// Объяснение: docs/campaign-map/render.md.

// Push-константы, ровно 128 байт. Раскладка совпадает с MapConstants в src/gpu/mod.rs.
layout(push_constant) uniform MapFrame {
    mat4 view_proj;
    vec4 eye;    // xyz — позиция камеры, w — время в секундах
    vec4 sun;    // xyz — направление на солнце, w — сила окраски владельцев 0..1
    vec4 map;    // x — сторона карты в мире, y — точек в текстурах, z — полуширина границы в пикселях, w — где начинается дымка
    uvec4 ids;   // x — выбранный регион, y — регион под курсором, z — режим показа, w — флаги
} frame;

const uint FLAG_FACTION_BORDERS = 1u;
const uint FLAG_REGION_LINES = 2u;
const uint FLAG_GLOW = 4u;

const uint VIEW_FINAL = 0u;
const uint VIEW_ALBEDO = 1u;
const uint VIEW_LIGHTMAP = 2u;
const uint VIEW_NORMALS = 3u;
const uint VIEW_REGIONS = 4u;
const uint VIEW_POLITICAL = 5u;

layout(set = 0, binding = 0) uniform sampler2D t_albedo;   // цвет земли, альфа 0 — вода
layout(set = 0, binding = 1) uniform sampler2D t_normal;   // запечённые нормали
layout(set = 0, binding = 2) uniform sampler2D t_light;    // R — тень от гор, G — затенение впадин
layout(set = 0, binding = 3) uniform usampler2D t_region;  // номер региона, 0 — море
layout(set = 0, binding = 4) uniform sampler2D t_border;   // R — расстояние до края региона, G — номер соседа

// Хак H-04: регион → владелец → цвет. Смена владельца — запись одного числа.
layout(std430, set = 0, binding = 5) readonly buffer Tables {
    uint owner[256];
    vec4 colour[256];
} tables;

const float BORDER_RANGE = 8.0; // совпадает с bake::BORDER_RANGE

#ifdef MAP_FRAGMENT
// Дальше — только для пиксельных шейдеров (fwidth есть только там).

// Четыре ближайшие точки текстуры регионов и их веса (как при билинейной
// фильтрации, только вручную: номера нельзя смешивать, а веса — можно).
struct Quad {
    uint id[4];
    float w[4];
};

Quad gather_regions(vec2 uv) {
    float n = frame.map.y;
    vec2 p = uv * n - 0.5;
    vec2 b = floor(p);
    vec2 f = p - b;
    ivec2 i = ivec2(b);
    int m = int(n) - 1;
    const ivec2 o[4] = ivec2[](ivec2(0, 0), ivec2(1, 0), ivec2(0, 1), ivec2(1, 1));
    float w[4] = float[]((1.0 - f.x) * (1.0 - f.y), f.x * (1.0 - f.y), (1.0 - f.x) * f.y, f.x * f.y);
    Quad q;
    for (int k = 0; k < 4; k++) {
        q.id[k] = texelFetch(t_region, clamp(i + o[k], ivec2(0), ivec2(m)), 0).r;
        q.w[k] = w[k];
    }
    return q;
}

// Ключ точки: сам регион или его владелец. Море — особый ключ.
uint key_of(uint id, bool by_owner) {
    return by_owner ? tables.owner[id] : id;
}

// Доля веса суши, у которой тот же ключ, что у ближайшей точки суши.
// Внутри области = 1, ровно на границе двух областей = 0,5. Море не
// считается: морской берег — не граница.
float same_share(Quad q, bool by_owner) {
    int best = -1;
    float land = 0.0;
    for (int k = 0; k < 4; k++) {
        if (q.id[k] != 0u) {
            land += q.w[k];
            if (best < 0 || q.w[k] > q.w[best]) best = k;
        }
    }
    if (best < 0) return 1.0;
    uint ref_key = key_of(q.id[best], by_owner);
    float same = 0.0;
    for (int k = 0; k < 4; k++) {
        if (q.id[k] != 0u && key_of(q.id[k], by_owner) == ref_key) same += q.w[k];
    }
    return same / max(land, 1e-5);
}

// Сколько пикселей от линии share = 0,5. Деление на fwidth делает толщину
// линии одинаковой в пикселях при любом приближении камеры.
float line_distance_px(float share) {
    return abs(share - 0.5) / max(fwidth(share), 1e-4);
}
#endif
