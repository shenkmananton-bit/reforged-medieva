#version 450
// Интерфейс: цветные прямоугольники в пикселях экрана (хак H-25: весь
// интерфейс одним вызовом). Буквы — тоже прямоугольники, по одному на пиксель шрифта.
layout(push_constant) uniform Screen {
    vec2 size;
} screen;

layout(location = 0) in vec4 i_rect;  // x, y, ширина, высота в пикселях
layout(location = 1) in vec4 i_color;

layout(location = 0) out vec4 v_color;

void main() {
    const vec2 corners[6] = vec2[](
        vec2(0.0, 0.0), vec2(1.0, 0.0), vec2(1.0, 1.0),
        vec2(0.0, 0.0), vec2(1.0, 1.0), vec2(0.0, 1.0));
    vec2 px = i_rect.xy + corners[gl_VertexIndex] * i_rect.zw;
    gl_Position = vec4(px / screen.size * 2.0 - 1.0, 0.0, 1.0);
    v_color = i_color;
}
