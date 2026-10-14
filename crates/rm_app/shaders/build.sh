#!/bin/sh
# Пересобирает SPIR-V из GLSL. Нужен glslangValidator (пакет glslang-tools
# или Vulkan SDK). Готовые .spv лежат в репозитории, поэтому для обычной
# сборки игры этот скрипт не нужен. Подробно: docs/app/render.md.
set -e
cd "$(dirname "$0")"
for s in mesh.vert mesh.frag ground.vert ground.frag ui.vert ui.frag; do
    glslangValidator -V --quiet "$s" -o "spv/$s.spv"
done
