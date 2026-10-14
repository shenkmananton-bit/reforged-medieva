// Общие данные кадра: матрица камеры, солнце, позиция камеры и туман.
// Передаются push-константами (128 байт хватает на всё, буфер не нужен).
layout(push_constant) uniform Frame {
    mat4 view_proj;
    vec4 sun;      // xyz — направление на солнце, w — половина размера поля
    vec4 eye;      // xyz — позиция камеры, w — дистанция начала тумана
    vec4 fog;      // rgb — цвет тумана (= цвет неба), a — длина перехода
} frame;

float fog_amount(vec3 world) {
    return clamp((distance(world, frame.eye.xyz) - frame.eye.w) / frame.fog.a, 0.0, 0.85);
}
