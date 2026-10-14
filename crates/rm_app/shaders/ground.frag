#version 450
#extension GL_GOOGLE_include_directive : require
#include "common.glsl"

layout(location = 0) in vec3 v_world;
layout(location = 0) out vec4 o_color;

float hash(vec2 p) {
    p = fract(p * vec2(123.34, 456.21));
    p += dot(p, p + 45.32);
    return fract(p.x * p.y);
}

float noise(vec2 p) {
    vec2 i = floor(p);
    vec2 f = fract(p);
    vec2 u = f * f * (3.0 - 2.0 * f);
    return mix(mix(hash(i), hash(i + vec2(1, 0)), u.x),
               mix(hash(i + vec2(0, 1)), hash(i + vec2(1, 1)), u.x), u.y);
}

void main() {
    vec2 p = v_world.xz;
    // Пятна травы разного оттенка (несколько октав шума) и мелкая «зернистость».
    float big = noise(p * 0.012) * 0.6 + noise(p * 0.05) * 0.4;
    vec3 grass = mix(vec3(0.16, 0.24, 0.07), vec3(0.28, 0.33, 0.11), big);
    grass = mix(grass, vec3(0.33, 0.29, 0.15), smoothstep(0.62, 0.8, noise(p * 0.02 + 17.0)) * 0.6);
    grass *= 0.92 + noise(p * 0.9) * 0.16;

    // Край поля битвы: дальше — затемнённая земля, за неё бегут разбитые отряды.
    float half_size = frame.sun.w;
    float edge = max(abs(p.x), abs(p.y)) - half_size;
    grass *= edge > 0.0 ? 0.6 : 1.0;
    grass = mix(grass, vec3(0.5, 0.45, 0.25), (1.0 - smoothstep(0.0, 1.5, abs(edge))) * 0.6);

    float light = max(normalize(frame.sun.xyz).y, 0.0) * 0.7 + 0.38;
    o_color = vec4(mix(grass * light, frame.fog.rgb, fog_amount(v_world)), 1.0);
}
