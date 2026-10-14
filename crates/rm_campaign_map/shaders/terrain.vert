#version 450
// Рельеф (H-01): сетка вершин уже приподнята по высотам на процессоре.
// Этот же шейдер используют проход рельефа и проход маски границ — поэтому
// глубина в них совпадает до бита (invariant), и маска ложится точно на рельеф.
#extension GL_GOOGLE_include_directive : require
#include "map_common.glsl"

layout(location = 0) in vec3 a_pos;
layout(location = 1) in vec2 a_uv;

layout(location = 0) out vec3 v_world;
layout(location = 1) out vec2 v_uv;

invariant gl_Position;

void main() {
    v_world = a_pos;
    v_uv = a_uv;
    gl_Position = frame.view_proj * vec4(a_pos, 1.0);
}
