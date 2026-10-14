#version 450
// Земля — один большой прямоугольник; трава рисуется в пиксельном шейдере.
#extension GL_GOOGLE_include_directive : require
#include "common.glsl"

layout(location = 0) out vec3 v_world;

void main() {
    const vec2 corners[6] = vec2[](
        vec2(-1.0, -1.0), vec2(1.0, -1.0), vec2(1.0, 1.0),
        vec2(-1.0, -1.0), vec2(1.0, 1.0), vec2(-1.0, 1.0));
    vec2 c = corners[gl_VertexIndex] * 3000.0;
    v_world = vec3(c.x, 0.0, c.y);
    gl_Position = frame.view_proj * vec4(v_world, 1.0);
}
