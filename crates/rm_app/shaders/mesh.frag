#version 450
#extension GL_GOOGLE_include_directive : require
#include "common.glsl"

layout(location = 0) in vec3 v_color;
layout(location = 1) in vec3 v_normal;
layout(location = 2) in float v_fog;
layout(location = 3) in float v_alpha;

layout(location = 0) out vec4 o_color;

void main() {
    // Освещение по Ламберту от одного солнца плюс рассеянный свет неба.
    vec3 n = normalize(v_normal);
    float light = max(dot(n, normalize(frame.sun.xyz)), 0.0) * 0.7 + 0.38;
    vec3 c = mix(v_color * light, frame.fog.rgb, v_fog);
    o_color = vec4(c, v_alpha);
}
